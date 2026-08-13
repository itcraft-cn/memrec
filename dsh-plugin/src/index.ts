/**
 * # MemRec DSH 原生插件
 *
 * 将 memrec（本地优先的 AI 记忆系统）深度集成进 DeepSeek Harness：
 *
 * 1. **原生工具**：`mr_add` / `mr_search` / `mr_get` / `mr_list` /
 *    `mr_delete` / `mr_stats` / `mr_dream`，Unix Socket 直连 memrecd，
 *    无中间子进程；工具执行时自动携带会话工作目录，项目隔离正确
 *    （MCP 模式不传 `working_dir`，项目隔离失效，见设计文档 2.4）；
 * 2. **项目记忆上下文注入**：模型请求前自动检索当前项目相关记忆，
 *    作为 plugin 消息注入，减少"想不起来"；
 * 3. **会话自动记录**：turn 结束时由 LLM 提取决策/知识/偏好写入 memrec。
 *
 * 所有功能均可通过配置开关；daemon 不可用时整体降级，不影响 DSH 会话。
 *
 * 插件形态：命名导出 `name` / `inject` / `apply`（无默认导出），
 * 符合 Harness 插件加载契约。
 */
import z from '@deepseek-ai/schemastery'
import type { Context } from '@deepseek-ai/cordis'
import { MemrecDaemon } from './daemon.js'
import { registerMemrecTools } from './tools.js'
import { installContextInjection } from './context.js'
import { installAutoRecord } from './lifecycle.js'

export const name = 'memrec'

/** 需要等待就绪的服务。 */
export const inject = ['tools', 'agents']

/** 插件配置类型。 */
export type Config = {
  /** memrecd Unix Socket 路径，默认 `~/.memrec/memrecd.sock`。 */
  socketPath?: string
  /** 是否注入项目记忆上下文（agent/pre-step 检索注入），默认 true。 */
  injectContext?: boolean
  /** 上下文注入每次检索条数，默认 5。 */
  contextTopK?: number
  /** 上下文注入最低相似度，默认 0.45。 */
  contextMinScore?: number
  /** 每 N 个 turn 注入一次上下文，默认 1。 */
  contextRefreshTurns?: number
  /** 是否启用会话自动记录（LLM 提取），默认 true。 */
  autoRecord?: boolean
  /** 自动记录 LLM provider（空则用会话 agent 的）。 */
  autoRecordProvider?: string
  /** 自动记录 LLM model（空则用会话 agent 的）。 */
  autoRecordModel?: string
  /** 每 N 个 turn 评估一次自动记录，默认 2。 */
  autoRecordInterval?: number
  /** 单会话自动记录上限，默认 20。 */
  autoRecordMaxPerSession?: number
}

/** 插件配置 schema（schemastery 校验）。 */
export const Config = z.object({
  socketPath: z.string().default(''),
  injectContext: z.boolean().default(true),
  contextTopK: z.number().step(1).min(1).max(20).default(5),
  contextMinScore: z.number().min(0).max(1).default(0.45),
  contextRefreshTurns: z.number().step(1).min(1).default(1),
  autoRecord: z.boolean().default(true),
  autoRecordProvider: z.string().default(''),
  autoRecordModel: z.string().default(''),
  autoRecordInterval: z.number().step(1).min(1).default(2),
  autoRecordMaxPerSession: z.number().step(1).min(0).default(20),
})

/**
 * 插件装配入口。
 * @param ctx - Cordis 上下文（tools/agents 已就绪）
 * @param config - 校验后的插件配置（默认值已填充）
 */
export function apply(ctx: Context, config: Config): void {
  const socketPath =
    config.socketPath && config.socketPath.length > 0
      ? config.socketPath
      : MemrecDaemon.defaultSocketPath()
  const daemon = new MemrecDaemon(socketPath)

  // 1. 原生 mr_* 工具
  registerMemrecTools(ctx, daemon)

  // 2. 项目记忆上下文注入
  installContextInjection(ctx, daemon, {
    enabled: config.injectContext !== false,
    topK: config.contextTopK ?? 5,
    minScore: config.contextMinScore ?? 0.45,
    refreshTurns: config.contextRefreshTurns ?? 1,
  })

  // 3. 会话自动记录
  installAutoRecord(
    ctx as never,
    daemon,
    {
      enabled: config.autoRecord !== false,
      provider: config.autoRecordProvider ?? '',
      model: config.autoRecordModel ?? '',
      interval: config.autoRecordInterval ?? 2,
      maxPerSession: config.autoRecordMaxPerSession ?? 20,
    },
  )
}
