# @memrec/dsh-plugin

MemRec（本地优先 AI 记忆系统）的 DeepSeek Harness 原生插件：Unix Socket 直连
memrecd 的原生工具 + 项目记忆上下文注入 + 会话自动记录。

## 能力

| 能力 | 说明 |
|---|---|
| 原生工具 | `mr_add` / `mr_search` / `mr_get` / `mr_list` / `mr_delete` / `mr_stats` / `mr_dream`，无 `mcp__` 前缀 |
| 项目隔离 | 工具执行自动携带会话工作目录（`working_dir`），memrecd 据此检测项目（git root + `.mr_pid`）。MCP 模式不传 `working_dir`，项目隔离失效，本插件修复此问题 |
| 上下文注入 | `agent/pre-step` 钩子：模型请求前自动检索当前项目相关记忆，以 plugin 消息注入，减少"想不起来" |
| 自动记录 | turn 结束时由 LLM 提取决策/知识/偏好/上下文写入 memrec（`source: inferred`），有频率与条数上限 |

## 安装

前置：memrecd 守护进程已运行（`memrecctl.sh start`）。

```bash
# 1. 构建
cd dsh-plugin
npm install --cache <workdir-cache>   # 沙箱环境可将 npm 缓存指到工作区
npm run build

# 2. 安装到目标 profile（如 web）
dsh plugin --profile web add /abs/path/to/memrec/dsh-plugin

# 3. 启用（$DSH_HOME/profiles/web/cordis.patch.yml 追加）
- insert:
    - id: memrec
      name: '/abs/path/to/memrec/dsh-plugin/lib/index.js'
      config:
        socketPath: '~/.memrec/memrecd.sock'
        injectContext: true
        autoRecord: true

# 4. 重启 DSH 进程（新插件依赖 node_modules 变更，需重启生效）
```

## 配置

| 字段 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `socketPath` | string | `~/.memrec/memrecd.sock` | memrecd socket 路径（支持 `~` 展开） |
| `injectContext` | boolean | true | 是否注入项目记忆上下文 |
| `contextTopK` | number | 5 | 每次注入的检索条数（1-20） |
| `contextMinScore` | number | 0.45 | 注入检索最低相似度（0-1） |
| `contextRefreshTurns` | number | 1 | 每 N 个 turn 注入一次 |
| `autoRecord` | boolean | true | 是否启用会话自动记录（LLM 提取） |
| `autoRecordProvider` | string | '' | LLM provider（空则用会话 agent 的） |
| `autoRecordModel` | string | '' | LLM model（空则用会话 agent 的） |
| `autoRecordInterval` | number | 2 | 每 N 个 turn 评估一次 |
| `autoRecordMaxPerSession` | number | 20 | 单会话自动记录上限 |

## 开发

```bash
npm run build   # tsc 编译到 lib/
npm test        # 构建 + node --test（含真实 memrecd 集成测试，需 daemon 运行）
```

集成测试直连本机 memrecd，会写入并清理一条带 `dsh-plugin-test` 标记的测试记忆。

## 设计

详见 `docs/design/dsh-plugin-design.md`。核心要点：

- 协议与 `mr-protocol` 对齐（JSON-RPC 2.0，`type` 内部标签，一次请求一个连接）；
- daemon 不可用时整体降级：工具返回错误文本、注入静默跳过、自动记录仅告警，不影响 DSH 会话；
- 自动记录的 LLM 调用为 fire-and-forget，不阻塞 turn 流程。

## 限制

- 自动记录依赖 `llm` 服务与模型凭据（沿用会话 agent 的 provider/model）；
- 上下文注入按 turn 节流，记忆检索本身耗时约几十毫秒（embedding 生成），
  由 memrecd 本地完成，不经过网络。
