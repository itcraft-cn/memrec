/**
 * # 项目记忆上下文注入
 *
 * 在模型请求前（`agent/pre-step`）自动检索当前项目相关的 memrec 记忆，
 * 作为 plugin 来源的 user 消息注入会话。模式与 `@deepseek-ai/dsh-time-context`
 * 一致：
 *
 * - 仅在新 step（`step === 1`）注入，避免同一 turn 内重复；
 * - 查询词取该 turn inbox 中最新一条用户消息文本；
 * - 检索结果按 `contextRefreshTurns` 节流，同一 turn 幂等；
 * - daemon 不可用、检索为空时静默跳过，不增加 token 成本；
 * - 注入消息 `source` 为 `{kind:'plugin', plugin:'memrec', form:'snapshot', sections:[...]}`，
 *   会话日志可完整回溯注入内容。
 */
import type { Agent, PreStepDecision } from '@deepseek-ai/dsh-agent'
import type { Context } from '@deepseek-ai/cordis'
import { createUserMessage, type UserMessage } from '@deepseek-ai/dsh-llm'
import type { MemrecDaemon } from './daemon.js'

/** 注入模块配置。 */
export interface ContextConfig {
  /** 是否启用注入。 */
  readonly enabled: boolean
  /** 每次注入的检索条数。 */
  readonly topK: number
  /** 注入检索最低相似度。 */
  readonly minScore: number
  /** 每 N 个 turn 注入一次。 */
  readonly refreshTurns: number
}

/** 渲染单条记忆。 */
function renderHit(hit: {
  score?: number
  memory_type?: string
  content_preview?: string
  tags?: string[]
  created_at?: string
}): string {
  const score = hit.score !== undefined ? hit.score.toFixed(2) : '?'
  const type = hit.memory_type ?? 'conversation'
  const tags = hit.tags && hit.tags.length > 0 ? ` #${hit.tags.join(' #')}` : ''
  const preview = (hit.content_preview ?? '').replace(/\s+/g, ' ').trim()
  const body = preview.length > 0 ? preview : '(内容为空)'
  return `- [${score}] (${type})${tags}\n  ${body}`
}

/**
 * 将 daemon 检索结果渲染为注入文本。
 *
 * 防御性过滤（服务端 `min_score` 仅作用于向量侧原始过滤，融合后的
 * 分数可能低于阈值；部分记忆 `content_preview` 为空、无信息量）：
 * - 过滤 `score < minScore` 的条目；
 * - 过滤 `content_preview` 为空的条目；
 * - 注入上限 8 条，控制 token 成本。
 */
export function renderContextText(result: unknown, minScore = 0): string {
  const results = (result as { results?: unknown[] })?.results
  if (!Array.isArray(results) || results.length === 0) return ''
  const kept = results
    .filter((r) => {
      const hit = r as { score?: unknown; content_preview?: unknown }
      const score = typeof hit.score === 'number' ? hit.score : 0
      if (score < minScore) return false
      const preview = typeof hit.content_preview === 'string' ? hit.content_preview.trim() : ''
      return preview.length > 0
    })
    .slice(0, 8)
  if (kept.length === 0) return ''
  const lines = kept.map((r) => renderHit(r as Parameters<typeof renderHit>[0]))
  return `## MemRec 项目记忆（自动检索）\n${lines.join('\n')}`
}

/** 从 user 消息块中提取纯文本。 */
function textOfUserMessage(message: UserMessage): string {
  const parts: string[] = []
  for (const block of message.content) {
    if (block.type === 'text') parts.push(block.text)
  }
  return parts.join('\n').trim()
}

/** 构造注入消息。 */
function buildInjection(text: string): UserMessage {
  return createUserMessage({
    content: [{ type: 'text', text }],
    source: {
      kind: 'plugin',
      plugin: 'memrec',
      form: 'snapshot',
      sections: [{ name: 'memrec-project-context', text }],
    },
  })
}

/**
 * 注册项目记忆上下文注入。
 * @param ctx - 插件上下文（`agents` 服务就绪后调用）
 * @param daemon - memrecd 客户端
 * @param config - 注入配置
 */
export function installContextInjection(
  ctx: Context,
  daemon: MemrecDaemon,
  config: ContextConfig,
): void {
  if (!config.enabled) return

  // 每个 agent 最后注入的 turn 号：按 (agentId) 记录，turn 单调递增
  const lastInjectedTurn = new Map<string, number>()
  const warned = new Set<string>()

  ctx.on(
    'agent/pre-step',
    async (payload, next) => {
      const decision: PreStepDecision = await next()
      if (decision.kind !== 'enter' || payload.signal.aborted) return decision
      if (payload.step !== 1) return decision

      const agentId = payload.agent.id
      const lastTurn = lastInjectedTurn.get(agentId) ?? -Infinity
      if (payload.turn - lastTurn < config.refreshTurns) return decision

      // 查询词：最新一条用户消息（过滤掉其他插件注入）
      const userText = [...payload.messages]
        .reverse()
        .map(textOfUserMessage)
        .find((t) => t.length > 0)
      if (userText === undefined || userText.length === 0) return decision

      // 查询词过长时截断（检索不需要全文）
      const query = userText.slice(0, 200)

      let result: unknown
      try {
        result = await daemon.call('search_memory', {
          query,
          top_k: Math.max(config.topK, 8),
          min_score: config.minScore,
          include_global: true,
          working_dir: payload.agent.session.header.cwd ?? process.cwd(),
        })
      } catch (err) {
        // daemon 不可用：每 agent 仅告警一次，静默降级
        if (!warned.has(agentId)) {
          warned.add(agentId)
          // eslint-disable-next-line no-console
          console.warn(
            `[memrec] 上下文注入失败（仅提示一次）：${err instanceof Error ? err.message : String(err)}`,
          )
        }
        return decision
      }

      const text = renderContextText(result, config.minScore)
      if (text.length === 0) return decision

      lastInjectedTurn.set(agentId, payload.turn)
      return {
        kind: 'enter',
        messages: [...decision.messages, buildInjection(text)],
      }
    },
    { prepend: true },
  )
}