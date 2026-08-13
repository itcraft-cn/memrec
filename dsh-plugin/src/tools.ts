/**
 * # memrec 原生工具定义
 *
 * 注册 7 个与 MCP 模式对齐的 `mr_*` 工具，但通过 Unix Socket 直连
 * memrecd，并修复 MCP 模式的项目隔离缺陷：工具执行时从
 * `exec.agent.session.header.cwd` 取得会话工作目录，作为 `working_dir`
 * 传给服务端，memrecd 据此自动检测项目 ID（git root + `.mr_pid`）。
 *
 * 每个工具的结果统一返回 JSON 值（`{type:'json'}`），render 投影为文本，
 * 与 memrec 返回结构保持一致，方便后续接入结构化消费。
 */
import type { Agent } from '@deepseek-ai/dsh-agent'
import { defineTool } from '@deepseek-ai/dsh-tools'
import type { ToolRunContext } from '@deepseek-ai/dsh-tools'
import type { JsonValue } from '@deepseek-ai/dsh-session'
import type { MemrecDaemon } from './daemon.js'
import { MemrecDaemonError } from './daemon.js'

/** 工具执行时可用的上下文派生。 */
export interface ToolEnv {
  /** 会话工作目录（memrec 项目检测依据），缺失时回退到进程 cwd。 */
  workingDir: string
}

/**
 * 从工具执行上下文提取环境信息。
 * @param exec - 工具执行上下文（含 agent）
 * @returns 派生环境
 */
export function envFromExec(exec: ToolRunContext): ToolEnv {
  const agent = exec.agent as Agent | undefined
  const cwd = agent?.session.header.cwd
  return { workingDir: cwd && cwd.length > 0 ? cwd : process.cwd() }
}

/** 将 daemon 返回值投影为模型可见文本。 */
function renderJson(value: unknown): string {
  if (value === undefined) return 'ok'
  return JSON.stringify(value, null, 2)
}

/** 工具调用参数基类。 */
interface ToolArgs {
  [key: string]: unknown
}

/**
 * 定义一个 memrec 工具。
 *
 * 经 `defineTool` 编译管道生成标准 ToolDefinition（输出 schema 为
 * `{type:'object', additionalProperties:true}`，保留 memrec 返回结构）。
 * @param name - 工具名
 * @param description - 模型可见描述
 * @param parameters - 参数 schema（dsh-tools DSL）
 * @param run - 执行函数：接收参数与环境，返回 daemon result
 */
export function defineMemrecTool(
  name: string,
  description: string,
  parameters: Record<string, unknown>,
  run: (args: Record<string, unknown>, env: ToolEnv) => Promise<unknown>,
) {
  return defineTool({
    name,
    description,
    parameters: parameters as never,
    output: {
      schema: { type: 'object', additionalProperties: true },
      render: (_args: never, value: JsonValue) => [
        { type: 'text' as const, text: renderJson(value) },
      ],
    },
    async execute(args: Record<string, unknown>, exec: ToolRunContext) {
      const env = envFromExec(exec)
      try {
        return await run(args, env)
      } catch (err) {
        if (err instanceof MemrecDaemonError) {
          // daemon 不可用等可辨识错误：返回错误文本而不是抛异常，
          // 避免模型把整个会话打挂。
          return { error: err.message }
        }
        throw err
      }
    },
  } as never)
}

/** 记忆类型枚举（与 mr_common::MemoryType 对齐）。 */
const MEMORY_TYPES = ['decision', 'knowledge', 'context', 'preference', 'conversation'] as const

/** 添加记忆。 */
function addTool(daemon: MemrecDaemon) {
  return defineMemrecTool(
    'mr_add',
    'Add a memory to MemRec (local-first AI memory with project isolation). ' +
      'Use for key decisions, learnings, user preferences, and project context. ' +
      'Memories are automatically attached to the current project (detected from the ' +
      'session working directory), unless is_global is true.',
    {
      content: {
        type: 'string',
        required: true,
        description: 'Memory content',
      },
      memory_type: {
        type: 'string',
        enum: [...MEMORY_TYPES],
        required: true,
        description:
          'Memory type. decision=key decisions, knowledge=learnings/facts, context=project info, preference=user prefs, conversation=dialogue',
      },
      tags: {
        type: 'array',
        items: { type: 'string' },
        description: "Tags for categorization. Use 'critical' for important, 'fact' for objective facts, 'best-practice' for patterns",
      },
      is_global: {
        type: 'boolean',
        description: 'If true, the memory is accessible from all projects (use for user preferences)',
      },
      source: {
        type: 'string',
        enum: ['user', 'system', 'inferred', 'external'],
        description: 'Source weight: user=highest, system=medium, inferred/external=lower',
      },
    },
    async (args, env) => {
      const content = String(args.content ?? '')
      if (content.trim().length === 0) {
        throw new MemrecDaemonError('mr_add: content 不能为空')
      }
      const result = await daemon.call('add', {
        content,
        memory_type: String(args.memory_type ?? 'conversation'),
        tags: Array.isArray(args.tags) ? args.tags.map(String) : [],
        is_global: Boolean(args.is_global),
        source: args.source !== undefined ? String(args.source) : undefined,
        working_dir: env.workingDir,
      })
      const memory = (result as { memory?: { id?: string } })?.memory
      return {
        memory_id: memory?.id ?? 'unknown',
        message: `Added memory: ${memory?.id ?? 'unknown'}`,
      }
    },
  )
}

/** 语义搜索。 */
function searchTool(daemon: MemrecDaemon) {
  return defineMemrecTool(
    'mr_search',
    'Search memories using semantic similarity (hybrid KNN+BM25 with MMR re-ranking). ' +
      'By default searches the current project plus global memories. ' +
      'Returns results scored by relevance (0-1).',
    {
      query: { type: 'string', required: true, description: 'Search query' },
      top_k: { type: 'integer', description: 'Max results to return', default: 10 },
      min_score: { type: 'number', description: 'Minimum similarity score (0-1)', default: 0.5 },
      project_only: { type: 'boolean', description: 'Search only the current project' },
      global_only: { type: 'boolean', description: 'Search only global memories' },
      cross_project: { type: 'boolean', description: 'Search across all projects' },
      memory_type: {
        type: 'string',
        enum: [...MEMORY_TYPES],
        description: 'Filter by memory type',
      },
    },
    async (args, env) => {
      const query = String(args.query ?? '')
      if (query.trim().length === 0) {
        throw new MemrecDaemonError('mr_search: query 不能为空')
      }
      const result = await daemon.call('search_memory', {
        query,
        top_k: Number(args.top_k ?? 10),
        min_score: Number(args.min_score ?? 0.5),
        project_only: Boolean(args.project_only),
        global_only: Boolean(args.global_only),
        cross_project: Boolean(args.cross_project),
        include_global: !args.project_only,
        memory_type: args.memory_type !== undefined ? String(args.memory_type) : undefined,
        working_dir: env.workingDir,
      })
      return result
    },
  )
}

/** 获取单条记忆。 */
function getTool(daemon: MemrecDaemon) {
  return defineMemrecTool(
    'mr_get',
    'Get a specific memory by ID, optionally merging chunked memories into full content.',
    {
      id: { type: 'string', required: true, description: 'Memory UUID' },
      merge: { type: 'boolean', description: 'Merge chunked memories into full content' },
    },
    async (args) => {
      const id = String(args.id ?? '')
      if (id.trim().length === 0) {
        throw new MemrecDaemonError('mr_get: id 不能为空')
      }
      return daemon.call('get', { id, merge: Boolean(args.merge) })
    },
  )
}

/** 列出记忆。 */
function listTool(daemon: MemrecDaemon) {
  return defineMemrecTool(
    'mr_list',
    'List memories with optional filtering.',
    {
      limit: { type: 'integer', description: 'Max memories to return', default: 20 },
      project_only: { type: 'boolean', description: 'List only the current project' },
      global_only: { type: 'boolean', description: 'List only global memories' },
    },
    async (args, env) => {
      return daemon.call('list', {
        limit: Number(args.limit ?? 20),
        project_only: Boolean(args.project_only),
        global_only: Boolean(args.global_only),
        working_dir: env.workingDir,
      })
    },
  )
}

/** 删除记忆（软删除）。 */
function deleteTool(daemon: MemrecDaemon) {
  return defineMemrecTool(
    'mr_delete',
    'Delete a memory by ID (soft delete).',
    {
      id: { type: 'string', required: true, description: 'Memory UUID' },
    },
    async (args) => {
      const id = String(args.id ?? '')
      if (id.trim().length === 0) {
        throw new MemrecDaemonError('mr_delete: id 不能为空')
      }
      return daemon.call('delete', { id })
    },
  )
}

/** 统计信息。 */
function statsTool(daemon: MemrecDaemon) {
  return defineMemrecTool(
    'mr_stats',
    'Get memory statistics (counts by type, project, etc.).',
    {},
    async () => daemon.call('stats'),
  )
}

/** Dream 记忆整合。 */
function dreamTool(daemon: MemrecDaemon) {
  return defineMemrecTool(
    'mr_dream',
    'Execute Dream memory integration: consolidate and compress older memories into summary memories. ' +
      'Bypasses time/session gates when force=true.',
    {
      force: {
        type: 'boolean',
        description: 'If true, bypass gate checks (time interval, session count)',
        default: false,
      },
    },
    async (args, env) => {
      return daemon.call('dream', {
        force: Boolean(args.force),
        working_dir: env.workingDir,
      })
    },
  )
}

/** 注册全部 memrec 工具。 */
export function registerMemrecTools(
  ctx: { tools: { register(definition: unknown): unknown } },
  daemon: MemrecDaemon,
): void {
  for (const tool of [
    addTool(daemon),
    searchTool(daemon),
    getTool(daemon),
    listTool(daemon),
    deleteTool(daemon),
    statsTool(daemon),
    dreamTool(daemon),
  ]) {
    ctx.tools.register(tool as never)
  }
}

export type { JsonValue }
