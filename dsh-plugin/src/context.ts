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
  /** 注入总 token 预算（服务端按记忆类型优先级分层裁剪）。 */
  readonly maxTokens: number
}

/** 常驻规则正文的字符上限：防止异常数据挤占上下文。 */
const MAX_RULE_CHARS = 1600
/** 规则集缓存刷新间隔（turn）。Dream 按天运行，30 个 turn 足以覆盖更新。 */
const RULE_CACHE_TURNS = 30

/** 单条注入结果的兼容形态：普通检索用 `content_preview`，分层检索用 `content`。 */
interface RawHit {
  score?: number
  memory_type?: string
  tags?: string[]
  content_preview?: string
  content?: string
  tier?: string
  token_count?: number
}

/** 渲染单条记忆。 */
function renderHit(hit: RawHit): string {
  const score = hit.score !== undefined ? hit.score.toFixed(2) : '?'
  const type = hit.memory_type ?? 'conversation'
  const tags = hit.tags && hit.tags.length > 0 ? ` #${hit.tags.join(' #')}` : ''
  // 分层结果带 tier 标注（full/truncated/summary/dense_proxy），便于判断保真度
  const tier = hit.tier !== undefined ? ` ${hit.tier}` : ''
  const raw = hit.content ?? hit.content_preview ?? ''
  const body = raw.replace(/\s+/g, ' ').trim()
  return `- [${score}] (${type}${tier})${tags}\n  ${body.length > 0 ? body : '(内容为空)'}`
}

/**
 * 将 daemon 检索结果渲染为注入文本。
 *
 * 服务端在 `max_total_tokens` 下已完成类型优先级 + 分层裁剪，客户端不再按条数
 * 硬截断（旧实现固定 8 条，会浪费预算或挤掉高价值的偏好记忆）；这里只做
 * 防御性过滤：
 * - 过滤 `score < minScore` 的条目（融合后分数可能低于向量侧阈值）；
 * - 过滤内容为空的条目；
 * - 保留 64 条上限，防止异常响应（实际上限由 top_k 与服务端预算共同决定）。
 */
export function renderContextText(result: unknown, minScore = 0): string {
  const results = (result as { results?: unknown[] })?.results
  if (!Array.isArray(results) || results.length === 0) return ''
  const kept = results
    .filter((r) => {
      const hit = r as RawHit
      const score = typeof hit.score === 'number' ? hit.score : 0
      if (score < minScore) return false
      const body = (hit.content ?? hit.content_preview ?? '').trim()
      return body.length > 0
    })
    .slice(0, 64)
  if (kept.length === 0) return ''
  const lines = kept.map((r) => renderHit(r as RawHit))
  return `## MemRec 项目记忆（自动检索，按类型优先级预算注入）\n${lines.join('\n')}`
}

/** 规则集记忆的最小形态。 */
interface RuleMemoryLike {
  content?: string
  tags?: string[]
}

/** 从 list 响应中取出记忆数组。 */
function memoriesOf(result: unknown): RuleMemoryLike[] {
  const list = (result as { memories?: unknown[] } | undefined)?.memories
  return Array.isArray(list) ? (list as RuleMemoryLike[]) : []
}

/**
 * 拉取当前项目的常驻规则集（项目 + 全局各一条）。
 *
 * 规则集由 Dream 的规则提炼阶段产出，标签为 `rule-set`，每作用域覆盖式更新。
 * 这里不复用语义检索：规则必须**无条件**进入上下文，不能依赖查询相似度命中。
 * 项目定位交给守护进程（`get_project_info` + `.mr_pid`），与检索侧口径一致。
 */
export async function fetchRuleSets(
  daemon: MemrecDaemon,
  cwd: string,
): Promise<RuleMemoryLike[]> {
  const global = await daemon
    .call('list', { tags: ['rule-set'], global_only: true, limit: 2 })
    .then(memoriesOf)
    .catch(() => [])

  const project = await daemon
    .call('get_project_info', { working_dir: cwd })
    .then((info) => (info as { project_id?: string } | undefined)?.project_id)
    .then((projectId) =>
      projectId === undefined
        ? []
        : daemon
            .call('list', {
              tags: ['rule-set'],
              project_only: true,
              project_id: projectId,
              limit: 2,
            })
            .then(memoriesOf)
            .catch(() => []),
    )
    .catch(() => [])

  return [...project, ...global]
}

/**
 * 把规则集渲染为注入文本。
 *
 * 只做长度约束与空内容过滤；规则集本身已由服务端覆盖式更新并限制条数，
 * 客户端不再按条数切割。
 */
export function renderRuleText(memories: RuleMemoryLike[], maxChars = MAX_RULE_CHARS): string {
  const kept: string[] = []
  let used = 0
  for (const memory of memories) {
    const body = (memory.content ?? '').trim()
    if (body.length === 0) continue
    if (used + body.length > maxChars) {
      const room = maxChars - used
      if (room > 40) kept.push(body.slice(0, room))
      break
    }
    kept.push(body)
    used += body.length
  }
  if (kept.length === 0) return ''
  return `## MemRec 常驻规则（Dream 从决策/偏好提炼，覆盖式更新）\n${kept.join('\n')}`
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
  // 每个 agent 最近一次拉取的常驻规则文本与轮次（规则变化频率远低于检索）
  const ruleCache = new Map<string, { turn: number; text: string }>()
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

      const cwd = payload.agent.session.header.cwd ?? process.cwd()

      // 常驻规则：不依赖查询命中，按 turn 间隔缓存；Dream 覆盖式更新后自动生效。
      const cached = ruleCache.get(agentId)
      if (cached === undefined || payload.turn - cached.turn >= RULE_CACHE_TURNS) {
        try {
          const ruleText = renderRuleText(await fetchRuleSets(daemon, cwd))
          ruleCache.set(agentId, { turn: payload.turn, text: ruleText })
        } catch {
          // 拉取失败不缓存，下一轮重试
        }
      }
      const rulesText = ruleCache.get(agentId)?.text ?? ''

      // 查询词：最新一条用户消息（过滤掉其他插件注入）
      const userText = [...payload.messages]
        .reverse()
        .map(textOfUserMessage)
        .find((t) => t.length > 0)

      let contextText = ''
      if (userText !== undefined && userText.length > 0) {
        // 查询词过长时截断（检索不需要全文）
        const query = userText.slice(0, 200)
        try {
          // 请求分层检索：服务端在总预算内按记忆类型优先级裁剪，
          // 返回带完整内容与层级标注（full/truncated/summary/dense_proxy）的结果。
          const result = await daemon.call('search_memory', {
            query,
            top_k: Math.max(config.topK, 24),
            min_score: config.minScore,
            include_global: true,
            max_total_tokens: config.maxTokens,
            working_dir: cwd,
          })
          contextText = renderContextText(result, config.minScore)
        } catch (err) {
          // daemon 不可用：每 agent 仅告警一次，静默降级
          if (!warned.has(agentId)) {
            warned.add(agentId)
            // eslint-disable-next-line no-console
            console.warn(
              `[memrec] 上下文注入失败（仅提示一次）：${err instanceof Error ? err.message : String(err)}`,
            )
          }
        }
      }

      const text = [rulesText, contextText].filter((s) => s.length > 0).join('\n\n')
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