/**
 * # 会话自动记录（turn/end 生命周期钩子）
 *
 * 在每个 turn 结束时，收集该 turn 的用户消息与助手回复，交给 LLM 提取
 * 值得长期记忆的内容（决策 / 知识 / 偏好 / 上下文），写入 memrec。
 *
 * 设计要点：
 *
 * - 事件源：`session/event` 的 `turn/start` / `user/message` /
 *   `assistant/message` / `turn/end`（emit 模式，不阻塞 turn 流程）；
 * - LLM 提取为 fire-and-forget：监听器立即返回，异步任务在后台完成，
 *   失败仅 warn，绝不影响会话；
 * - 频率控制：`interval`（每 N 个 turn 评估一次）与
 *   `maxPerSession`（单会话自动记录上限）；
 * - provider/model 选择：配置显式指定优先，否则取该会话 agent 的
 *   options（与主对话同模型）；均不可用则禁用自动记录并告警一次；
 * - 写入 memrec 时携带 `working_dir`（会话 cwd），项目隔离正确。
 */
import type { Context } from '@deepseek-ai/cordis'
import type { AgentRegistry } from '@deepseek-ai/dsh-agent'
import type { LlmRuntime, Message, StreamChunk } from '@deepseek-ai/dsh-llm'
import type { MemrecDaemon } from './daemon.js'

/** 自动记录配置。 */
export interface AutoRecordConfig {
  /** 是否启用。 */
  readonly enabled: boolean
  /** 显式 provider 路由（空则用 agent 的）。 */
  readonly provider: string
  /** 显式 model（空则用 agent 的）。 */
  readonly model: string
  /** 每 N 个 turn 评估一次。 */
  readonly interval: number
  /** 单会话自动记录上限。 */
  readonly maxPerSession: number
}

/** 提取结果条目（LLM 输出）。 */
export interface ExtractedMemory {
  memory_type: 'decision' | 'knowledge' | 'preference' | 'context' | string
  content: string
  tags?: string[]
}

/** 提取用 system 提示。 */
const EXTRACT_SYSTEM_PROMPT = [
  '你是 MemRec 记忆系统的提取器。分析下面的对话，识别其中值得长期记忆的内容：',
  '1. decision：用户或助手做出的关键技术/业务决策；',
  '2. knowledge：新学到的客观知识、事实、最佳实践；',
  '3. preference：用户的偏好、习惯、工作方式；',
  '4. context：项目的重要背景信息。',
  '仅输出 JSON 数组，格式：',
  '[{"memory_type":"decision|knowledge|preference|context","content":"...","tags":["..."]}]',
  '没有值得记录的内容时输出 []。不要输出 JSON 之外的任何文本。',
].join('\n')

/** 从消息块提取纯文本。 */
function textOfBlocks(blocks: readonly { type: string; text?: string }[]): string {
  const parts: string[] = []
  for (const block of blocks) {
    if (block.type === 'text' && typeof block.text === 'string') parts.push(block.text)
  }
  return parts.join('\n').trim()
}

/** 消息文本是否过短（没有信息量）。 */
function isMeaningful(text: string): boolean {
  const t = text.replace(/\s+/g, '')
  return t.length >= 8
}

/** 从 LLM 输出中容错解析 JSON 数组。 */
export function parseExtraction(raw: string): ExtractedMemory[] {
  let text = raw.trim()
  // 提取 ```json 围栏内的内容
  const fence = text.match(/```(?:json)?\s*([\s\S]*?)\s*```/)
  if (fence !== null && fence[1] !== undefined) text = fence[1].trim()
  try {
    const parsed = JSON.parse(text)
    if (!Array.isArray(parsed)) return []
    return parsed.filter(
      (item): item is ExtractedMemory =>
        typeof item === 'object' &&
        item !== null &&
        typeof (item as ExtractedMemory).content === 'string',
    )
  } catch {
    return []
  }
}

/** 拼接 LLM 流式输出。 */
async function collectStream(stream: AsyncIterable<StreamChunk>): Promise<string> {
  let text = ''
  for await (const chunk of stream) {
    const delta = (chunk as { delta?: { text?: string } }).delta?.text
    if (typeof delta === 'string') text += delta
  }
  return text
}

/**
 * 注册会话自动记录。
 * @param ctx - 插件上下文（`agents`、`llm` 服务就绪后调用）
 * @param daemon - memrecd 客户端
 * @param config - 自动记录配置
 */
export function installAutoRecord(
  ctx: Context,
  daemon: MemrecDaemon,
  config: AutoRecordConfig,
): void {
  if (!config.enabled) return

  const logger = ctx.logger

  // 每个会话的 turn 内消息缓冲
  const turnMessages = new Map<string, { turn: number; parts: string[] }>()
  const recordedCount = new Map<string, number>()
  const lastEvaluatedTurn = new Map<string, number>()
  let warnedProvider = false

  const warn = (message: string) => {
    if (logger !== undefined) logger.warn(message)
  }

  ctx.on('session/event', (session, event) => {
    const sessionId = session.id
    switch (event.type) {
      case 'turn/start': {
        turnMessages.set(sessionId, { turn: event.data.turn, parts: [] })
        break
      }
      case 'user/message': {
        // 仅人类直接输入计入自动记录（插件注入、子代理上下文不算）
        if (event.data.source.kind === 'user') {
          const entry = turnMessages.get(sessionId)
          if (entry !== undefined) {
            const text = textOfBlocks(event.data.content)
            if (isMeaningful(text)) entry.parts.push(`用户：${text}`)
          }
        }
        break
      }
      case 'assistant/message': {
        const entry = turnMessages.get(sessionId)
        if (entry !== undefined && entry.turn === event.data.turn) {
          const text = textOfBlocks(event.data.message.content)
          if (isMeaningful(text)) entry.parts.push(`助手：${text.slice(0, 4000)}`)
        }
        break
      }
      case 'turn/end': {
        const entry = turnMessages.get(sessionId)
        turnMessages.delete(sessionId)
        if (entry === undefined || entry.parts.length === 0) return

        const lastEval = lastEvaluatedTurn.get(sessionId) ?? -Infinity
        if (event.data.turn - lastEval < config.interval) return
        lastEvaluatedTurn.set(sessionId, event.data.turn)

        const count = recordedCount.get(sessionId) ?? 0
        if (count >= config.maxPerSession) return

        const transcript = entry.parts.join('\n\n').slice(0, 8000)
        void extractAndRecord(daemon, config, session, transcript, warn)
          .then((added) => {
            if (added > 0) recordedCount.set(sessionId, (recordedCount.get(sessionId) ?? 0) + added)
          })
          .catch((err) => {
            warn(`[memrec] 自动记录失败：${err instanceof Error ? err.message : String(err)}`)
          })
        break
      }
      default:
        break
    }
  })

  /**
   * LLM 提取并写入 memrec（fire-and-forget 主体）。
   * @returns 实际写入的条数
   */
  async function extractAndRecord(
    daemonInner: MemrecDaemon,
    configInner: AutoRecordConfig,
    session: import('@deepseek-ai/dsh-session').Session,
    transcript: string,
    log: (message: string) => void,
  ): Promise<number> {
    const agents = ctx.get('agents') as AgentRegistry | undefined
    const llm = ctx.get('llm') as LlmRuntime | undefined
    if (llm === undefined) {
      if (!warnedProvider) {
        warnedProvider = true
        log('[memrec] 自动记录未启用：llm 服务不可用')
      }
      return 0
    }

    const agent = agents?.get(session.id)
    const provider = configInner.provider.length > 0 ? configInner.provider : agent?.options.provider
    const model = configInner.model.length > 0 ? configInner.model : agent?.options.model
    if (provider === undefined || model === undefined) {
      if (!warnedProvider) {
        warnedProvider = true
        log(
          '[memrec] 自动记录未启用：无法确定 provider/model，请在插件配置中设置 autoRecordProvider/autoRecordModel',
        )
      }
      return 0
    }

    const messages: Message[] = [
      {
        role: 'user',
        content: [{ type: 'text', text: transcript }],
        source: { kind: 'user' },
      } as Message,
    ]

    const stream = llm.stream({
      provider,
      model,
      messages,
      system: EXTRACT_SYSTEM_PROMPT,
      maxTokens: 512,
      temperature: 0,
    })

    const raw = await collectStream(stream)
    const extracted = parseExtraction(raw)
    if (extracted.length === 0) return 0

    let added = 0
    for (const item of extracted.slice(0, 5)) {
      const content = item.content.trim()
      if (content.length === 0) continue
      try {
        await daemonInner.call('add', {
          content,
          memory_type: item.memory_type,
          tags: Array.isArray(item.tags) ? item.tags.map(String).slice(0, 8) : [],
          source: 'inferred',
          working_dir: session.header.cwd ?? process.cwd(),
        })
        added++
      } catch (err) {
        log(`[memrec] 自动记录写入失败：${err instanceof Error ? err.message : String(err)}`)
      }
    }
    if (added > 0) {
      log(`[memrec] 自动记录：本次 turn 写入 ${added} 条记忆`)
    }
    return added
  }
}
