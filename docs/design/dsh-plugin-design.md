# DSH 深度集成插件设计（上策）

## 1. 背景与目标

memrec 已支持 MCP 模式（`memrec --mcp`，stdio 传输），可被 Claude Code、Codex 等
MCP 客户端直接调用。DeepSeek Harness（DSH）是 Cordis 插件体系，内置
`@deepseek-ai/dsh-mcp-client` 可桥接 MCP server，但存在局限：

- 工具名为 `mcp__memrec__mr_add` 形式，前缀占用 token；
- MCP 层仅桥接 tools，resources 不被消费；
- MCP 层不传 `working_dir`，而 memrecd 是独立守护进程，导致项目隔离失效
  （见下文 2.4 分析）；
- 无 DSH 生命周期集成：无法自动记录、无法注入项目记忆上下文。

本设计实现 memrec 原生 DSH 插件（npm 包，仓库内 `dsh-plugin/` 目录），提供：

1. 原生 `mr_*` 工具（Unix Socket 直连 memrecd，无中间子进程）；
2. 项目记忆上下文注入（`agent/pre-step` 钩子，模型请求前自动检索注入）；
3. 会话自动记录（`session/event` turn/end 钩子，LLM 提取决策/偏好/知识写入 memrec）。

## 2. 现状分析

### 2.1 memrec 架构

```
memrec CLI ──(JSON-RPC, Unix Socket)──> memrecd 守护进程 ──> RocksDB + 向量 + Tantivy
memrec --mcp (MCP stdio server) ──同样经 Unix Socket 调 memrecd
```

- Socket 路径：`~/.memrec/memrecd.sock`
- 协议：JSON-RPC 2.0，请求 `{"jsonrpc":"2.0","method":"add","params":{"type":"add",...},"id":1}`
- 一次请求一个连接：连接 → 写请求 → `shutdown()` 关写端 → 读响应至 EOF

### 2.2 DSH 插件机制（Cordis）

- 插件是导出 `name` / `inject` / `apply` 的模块（命名导出，无默认导出）；
- `apply(ctx, config)` 中 `ctx.tools.register(defineTool({...}))` 注册工具；
- 工具定义：`defineTool({name, description, parameters, output: {schema, render}, execute(args, exec)})`；
- `exec.agent.session.header.cwd` 可取得会话工作目录（`ToolRunContext` 携带 `agent`）；
- 上下文注入标准模式（参考 `@deepseek-ai/dsh-time-context`）：
  `ctx.on('agent/pre-step', async ({agent, turn, step, signal}, next) => ...)`，
  向 `decision.messages` 追加 `createUserMessage({content, source:{kind:'plugin', plugin, form:'snapshot', sections}})`；
- 会话事件：`ctx.on('session/event', (session, event) => ...)`，事件含
  `turn/start`、`turn/end`、`user/message`、`assistant/message` 等；
- LLM 调用：`ctx.llm.stream({provider, model, messages, system})`。

### 2.3 模块解析

- Loader `baseUrl` 为 profile 目录；插件模块名（绝对路径或包名）由 loader 解析；
- `$DSH_HOME/profiles/node_modules` 是 flat fallback 目录，软链了安装内的全部
  内置包，插件代码 `import '@deepseek-ai/dsh-tools'` 经 parent-walk 可命中；
- 安装方式：`dsh plugin --profile web add <本地包路径>`（转发 pnpm），
  或 patch 中 `insert` 本地编译产物（绝对路径）。

### 2.4 项目隔离缺陷（MCP 模式的已知问题）

`mr-protocol` 中 `AddParams` / `SearchMemoryParams` 均支持 `working_dir` 字段，
memrecd 服务端据此调用 `detect_project_id(working_dir)`（git root + `.mr_pid`）。
但 MCP 层（`memrec/src/mcp/mod.rs`）构造参数时**未传 working_dir**，而 MCP
stdio server 由客户端 spawn、memrecd 又是独立进程，因此：

- MCP 模式下的 add/search 项目隔离实际失效（全部落入默认项目或全局）；
- CLI 模式（`memrec add/search`）由客户端从自身 cwd 检测并传递，正常工作。

DSH 原生插件通过 `exec.agent.session.header.cwd` 或注入钩子的 `agent.session.header.cwd`
传递 working_dir，**项目隔离天然正确**，这是相对 MCP 模式的实质性增强。

## 3. 插件设计

### 3.1 包结构

```
dsh-plugin/
├── package.json          # name: @memrec/dsh-plugin, type: module, main: lib/index.js
├── tsconfig.json
├── README.md
├── src/
│   ├── index.ts          # 入口：Config schema + apply 装配
│   ├── daemon.ts         # Unix Socket JSON-RPC 客户端
│   ├── tools.ts          # 7 个 mr_* 原生工具
│   ├── context.ts        # agent/pre-step 项目记忆注入
│   └── lifecycle.ts      # turn/end 自动记录（LLM 提取）
└── lib/                  # tsc 构建产物
```

### 3.2 配置（schemastery schema）

| 字段 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `socketPath` | string | `~/.memrec/memrecd.sock` | memrecd socket 路径 |
| `injectContext` | boolean | true | 是否注入项目记忆上下文 |
| `contextTopK` | number | 5 | 每次注入的检索条数 |
| `contextMinScore` | number | 0.45 | 注入检索最低分 |
| `contextRefreshTurns` | number | 1 | 每 N 个 turn 注入一次 |
| `autoRecord` | boolean | true | turn 结束自动提取记录 |
| `autoRecordProvider` | string | '' | LLM provider（空则用 agent 的） |
| `autoRecordModel` | string | '' | LLM model（空则用 agent 的） |
| `autoRecordInterval` | number | 2 | 每 N 个 turn 评估一次 |
| `autoRecordMaxPerSession` | number | 20 | 单会话自动记录上限 |

### 3.3 daemon.ts — JSON-RPC 客户端

- `MemrecDaemon` 类：`call(method, params)`，一次请求一个连接；
- 连接失败（socket 不存在 / daemon 未运行）抛出可辨识错误，调用方降级；
- 响应解析：`{jsonrpc, result?, error?, id}`，error 转异常；
- 方法名映射与 `mr-protocol::RequestAction` 一致（add/get/update/delete/search/
  list/tag/search_memory/tiered_search/get_project_info/get_version/dream/
  project_*/config_*/stats/scene_*），参数内嵌 `type` 内部标签；
- 提供 `ping()` 探测 daemon 可用性（连接测试）。

### 3.4 tools.ts — 原生工具

7 个工具与 MCP 层 schema 对齐，并增加项目隔离增强：

| 工具 | 参数 | working_dir 来源 |
|---|---|---|
| `mr_add` | content, memory_type, tags, is_global, source | `exec.agent.session.header.cwd` |
| `mr_search` | query, top_k, min_score, project_only, global_only, cross_project, memory_type | 同上 |
| `mr_get` | id, merge | — |
| `mr_list` | limit, project_only, global_only | cwd |
| `mr_delete` | id | — |
| `mr_stats` | — | — |
| `mr_dream` | force | cwd |

- `output.schema` 使用 `{type:'json'}`（保持与 memrec 返回结构一致），
  render 投影为文本（`JSON.stringify` 或格式化）；
- daemon 不可用时返回明确错误信息（不抛未捕获异常）。

### 3.5 context.ts — 项目记忆上下文注入

- 监听 `agent/pre-step`（`{prepend: true}` 注册，仅 `step === 1` 时注入）；
- 查询词：取该 turn inbox 中最新一条 user 消息文本；
- 检索：`search_memory({query, working_dir, top_k: contextTopK, min_score: contextMinScore, include_global: true})`；
- 注入：`createUserMessage({content:[{type:'text', text}], source:{kind:'plugin', plugin:'memrec', form:'snapshot', sections:[{name:'memrec-project-context', text}]}})`；
- 缓存：按 `(agent.id, turn)` 幂等，turn 内只注入一次；`contextRefreshTurns` 控制注入频率；
- 降级：daemon 不可用时静默跳过（每会话首错打一次 warn 日志）；
- 检索结果为空时不注入（避免空消息占 token）；
- 注入文本格式：

```
## MemRec 项目记忆（自动检索）
- [0.87] (decision) 选择JWT认证方案 #auth #critical
  ...
```

### 3.6 lifecycle.ts — 自动记录

- 监听 `session/event` 的 `turn/end`（emit 模式，不阻塞 turn）；
- 收集该 turn 内 `user/message`（`source.kind === 'user'`）与 `assistant/message` 文本；
- 频率控制：`autoRecordInterval`（每 N 个 turn 评估一次）、`autoRecordMaxPerSession`；
- LLM 提取（fire-and-forget，失败仅 warn）：
  - 请求：`ctx.llm.stream({provider: config.provider || agent.options.provider, model: config.model || agent.options.model, messages, system})`；
  - system 提示要求输出 JSON 数组 `[{memory_type, content, tags}]`，无则 `[]`；
- 解析容错：提取 ```` ```json ```` 围栏，JSON.parse 失败降级为不记录；
- 写入：`daemon.call('add', {content, memory_type, tags, working_dir})`；
- `agent` 从 `session` 关联获取（`session` 事件载荷不含 agent；通过
  `agentRegistry` / `ctx.agents` 按 sessionId 查找，或缓存注入钩子已持有的引用）。

### 3.7 index.ts — 装配

- `Config` 用 schemastery `z.object(...)` 校验；
- `inject = ['tools']`（工具注册需要）；上下文注入需要 `agents`；
  自动记录需要 `llm`（可选注入，缺失时自动记录降级关闭）；
- `apply(ctx, config)` 顺序装配：daemon → tools → context → lifecycle。

## 4. 非功能设计

| 维度 | 设计 |
|---|---|
| 性能 | 每次检索一个 socket 连接（复用 memrec 既有协议，无额外进程）；注入/记录均有频率限制；`contextTopK=5` 控制 token 成本 |
| 可用性 | daemon 未运行/连接失败全部降级：工具返回错误文本、注入静默跳过、记录仅 warn，不中断会话 |
| 可观测性 | 关键路径 `ctx.logger` warn/info 日志；工具结果自带状态信息 |
| 安全 | 仅访问本机 socket；不读取环境凭据；注入文本截断（单条 preview 上限） |
| 可扩展 | daemon.ts 方法映射覆盖全部 RequestAction，后续可扩展更多工具（project、scene、tiered_search） |

## 5. 安装与启用

```bash
# 构建
cd dsh-plugin && npm install && npm run build

# 安装到 web profile（转发 pnpm）
dsh plugin --profile web add /disk2/helly_data/code/rust/memrec/dsh-plugin

# patch 启用（$DSH_HOME/profiles/web/cordis.patch.yml）
# - id: memrec
#   name: '@memrec/dsh-plugin'
#   config:
#     socketPath: '~/.memrec/memrecd.sock'
#     autoRecord: true
```

## 6. 分阶段计划

| 阶段 | 内容 | 验收 |
|---|---|---|
| 1 | daemon 客户端 + 7 工具 | node 单测直连真实 memrecd，7 工具可用 |
| 2 | 上下文注入 | 会话中模型可见注入记忆，token 可控 |
| 3 | 自动记录 | turn 结束自动写入 memrec，频率受限 |
| 4 | 安装启用 | profile 加载成功，工具列表出现 mr_* |
| 5 | 文档 | README + MANUAL_cn 章节 |

## 7. 风险与对策

| 风险 | 对策 |
|---|---|
| DSH rc 版本 API 变动 | 插件对内置包仅依赖稳定公开面（tools/agents/session/llm），版本跟随 |
| LLM 提取成本 | 默认每 2 turn 评估一次、上限 20 条/会话，可配置关闭 |
| 注入记忆占用上下文 | 默认 5 条、min_score 0.45、单条 preview 截断，可配置关闭 |
| patch 热重载限制 | 新插件依赖 node_modules 变更，需要重启 DSH 进程生效；文档注明 |
