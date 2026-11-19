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
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'

import type { Agent } from '@deepseek-ai/dsh-agent'
import { defineTool } from '@deepseek-ai/dsh-tools'
import type { ToolRunContext } from '@deepseek-ai/dsh-tools'
import type { MemrecDaemon } from './daemon.js'
import { MemrecDaemonError } from './daemon.js'

/** 项目标记文件名，与 memrec 守护进程及 CLI 保持一致。 */
const PROJECT_MARKER = '.mr_pid'

/**
 * 无损 JSON 值类型。
 *
 * DSH 0.2.2 起 `JsonValue` 不再由 `dsh-session`/`dsh-tools` 根入口导出，
 * 而从 `@deepseek-ai/dsh-util-values` 提供。本插件只需一个结构等价
 * 的别名用于 render 签名，避免为一个纯类型引入额外包依赖。
 */
type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue }

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
      max_total_tokens: {
        type: 'integer',
        description:
          'Token budget. When set (>0), returns tiered results sized to fit, prioritized by memory type',
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
        max_total_tokens:
          args.max_total_tokens !== undefined && Number(args.max_total_tokens) > 0
            ? Number(args.max_total_tokens)
            : undefined,
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

/**
 * 从项目根目录的 `.mr_pid` 标记文件解析项目 UUID。
 *
 * 服务端 `ListParams` 只有 `project_id`，**没有** `working_dir`；当
 * `project_only=true` 而 `project_id` 缺失时，服务端会退化为"匹配任意
 * 非空项目"，导致跨项目结果混入（CLI 与 MCP 都是客户端先解析再传入）。
 *
 * 查找策略：自 `startDir` 逐级向上（与守护进程写入位置一致，即 git 根），
 * 命中 `.mr_pid` 即解析返回；遇到 `.git` 目录或文件系统根即停止。
 *
 * @param startDir - 起始目录（会话工作目录）
 * @returns 项目 UUID 字符串；未注册或格式异常返回 null
 */
export function readProjectId(startDir: string): string | null {
  let dir = startDir
  for (let depth = 0; depth < 64; depth++) {
    try {
      const content = readFileSync(join(dir, PROJECT_MARKER), 'utf8')
      for (const line of content.split('\n')) {
        const [key, value] = line.split('=', 2)
        if (key !== undefined && key.trim() === 'memrec_project_id' && value !== undefined) {
          const id = value.trim()
          if (id.length > 0) return id
        }
      }
    } catch {
      // 该层没有标记文件，继续向上
    }
    // 到达 git 根（标记文件的常规位置）或文件系统根即停止
    if (existsSync(join(dir, '.git'))) break
    const parent = dirname(dir)
    if (parent === dir) break
    dir = parent
  }
  return null
}

/**
 * `mr_list` / `mr_timeline` 共用的参数 schema。
 *
 * 字段与 MCP 的 `mr_list`/`mr_timeline` 对齐：分页（limit/skip）、范围
 * （project_only/global_only）、过滤（tags/memory_type/include_deleted）、
 * 原始视图（raw/with_dream）。
 */
const LIST_PARAMETERS = {
  limit: { type: 'integer', description: 'Max memories to return', default: 20 },
  skip: { type: 'integer', description: 'Skip the first N memories (pagination)', default: 0 },
  project_only: { type: 'boolean', description: 'List only the current project' },
  global_only: { type: 'boolean', description: 'List only global memories' },
  tags: {
    type: 'array',
    items: { type: 'string' },
    description: 'Only memories carrying ALL of these tags',
  },
  memory_type: {
    type: 'string',
    enum: [...MEMORY_TYPES],
    description: 'Filter by memory type',
  },
  include_deleted: {
    type: 'boolean',
    description: 'Include soft-deleted raw records',
    default: false,
  },
  raw: {
    type: 'boolean',
    description:
      'Raw storage view: read the storage layer directly, including records Dream soft-deleted as merge sources',
    default: false,
  },
  with_dream: {
    type: 'boolean',
    description: 'With raw=true, also return Dream merge outputs (metadata.source_ids producers)',
    default: false,
  },
} as const

/**
 * 构造 `List` 请求参数（`mr_list` / `mr_timeline` 共用）。
 *
 * 项目范围必须在客户端解析成 `project_id`：服务端 `ListParams` 没有
 * `working_dir`，`project_only` 缺 `project_id` 时会退化为匹配任意项目。
 *
 * @param args - 模型提供的工具参数
 * @param env - 执行环境（会话工作目录）
 * @param order - 排序方式（`storage` 供 list，`created_desc` 供 timeline）
 * @param toolName - 报错信息中的工具名
 */
function buildListParams(
  args: Record<string, unknown>,
  env: ToolEnv,
  order: 'storage' | 'created_desc',
  toolName: string,
): Record<string, unknown> {
  const projectOnly = Boolean(args.project_only)
  const projectId = projectOnly ? readProjectId(env.workingDir) : null
  if (projectOnly && projectId === null) {
    throw new MemrecDaemonError(
      `${toolName}: 目录 ${env.workingDir} 未注册为 memrec 项目（未找到 .mr_pid），无法按项目过滤；请先在该项目写入一条非全局记忆`,
    )
  }
  return {
    limit: Number(args.limit ?? 20),
    skip: Number(args.skip ?? 0),
    project_only: projectOnly,
    global_only: Boolean(args.global_only),
    project_id: projectId,
    tags: Array.isArray(args.tags) ? args.tags.map(String) : [],
    memory_type: args.memory_type !== undefined ? String(args.memory_type) : undefined,
    include_deleted: Boolean(args.include_deleted),
    order,
    raw: Boolean(args.raw),
    with_dream: Boolean(args.with_dream),
  }
}

/** 列出记忆（存储顺序）。 */
function listTool(daemon: MemrecDaemon) {
  return defineMemrecTool(
    'mr_list',
    'List memories with optional filtering (storage order). Use mr_timeline for chronological recency.',
    LIST_PARAMETERS as unknown as Record<string, unknown>,
    async (args, env) => {
      return daemon.call('list', buildListParams(args, env, 'storage', 'mr_list'))
    },
  )
}

/** 按创建时间倒序浏览（时间线）。 */
function timelineTool(daemon: MemrecDaemon) {
  return defineMemrecTool(
    'mr_timeline',
    'Browse memories newest-first by creation time. Use this to answer "what happened recently" ' +
      'or "where did we leave off" instead of searching by relevance. Supports the same filters ' +
      'as mr_list, including raw / with_dream for the raw record view.',
    LIST_PARAMETERS as unknown as Record<string, unknown>,
    async (args, env) => {
      return daemon.call('list', buildListParams(args, env, 'created_desc', 'mr_timeline'))
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
      'It may merge, rewrite, or soft-delete source memories, so call it only when the user explicitly ' +
      'asks to consolidate or clean up memories — and preview with dry_run=true first.',
    {
      force: {
        type: 'boolean',
        description:
          'If true, bypass gate checks (time interval, new-memory count). Do not use force instead of normal gating',
        default: false,
      },
      dry_run: {
        type: 'boolean',
        description: 'Preview only: compute what Dream would do without writing, deleting, or adding edges',
        default: false,
      },
    },
    async (args) => {
      // DreamParams 没有 working_dir 字段（整合作用于全局/全部项目），
      // 不额外传字段，避免依赖服务端对未知字段的容忍行为。
      return daemon.call('dream', {
        force: Boolean(args.force),
        dry_run: Boolean(args.dry_run),
      })
    },
  )
}

/** 查询 Dream 运行审计报告。 */
function dreamReportTool(daemon: MemrecDaemon) {
  return defineMemrecTool(
    'mr_dream_report',
    'Inspect recent Dream runs: candidates, clusters, edges, gate outcomes, and quality metrics. ' +
      'Use it to audit what a consolidation did or why a run was skipped.',
    {
      last: {
        type: 'integer',
        description: 'Return the most recent N runs (default 10, max 100)',
        default: 10,
      },
      offset: {
        type: 'integer',
        description: 'Offset for pagination',
        default: 0,
      },
    },
    async (args) => {
      return daemon.call('dream_report', {
        last: Number(args.last ?? 10),
        offset: Number(args.offset ?? 0),
      })
    },
  )
}

/** 注册全部 memrec 工具。 */
/**
 * 注册全部 memrec 工具。
 *
 * 共 9 个：读写 5 个（add/search/get/list/delete）、统计 1 个（stats）、
 * 时间线 1 个（timeline）、Dream 2 个（dream/dream_report）。
 */
export function registerMemrecTools(
  ctx: { tools: { register(definition: unknown): unknown } },
  daemon: MemrecDaemon,
): void {
  for (const tool of [
    addTool(daemon),
    searchTool(daemon),
    getTool(daemon),
    listTool(daemon),
    timelineTool(daemon),
    deleteTool(daemon),
    statsTool(daemon),
    dreamTool(daemon),
    dreamReportTool(daemon),
  ]) {
    ctx.tools.register(tool as never)
  }
}

export type { JsonValue }
