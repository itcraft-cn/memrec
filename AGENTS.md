# MemRec

## AI记忆持久化系统，为AI CLI工具提供跨会话记忆恢复、知识库积累、对话历史存档能力。

## 技术栈

- **语言**: Rust
- **运行时**: Tokio async runtime
- **存储**: RocksDB + 自定义向量存储
- **通信**: Unix Socket + JSON-RPC 2.0
- **CLI**: clap

## 项目结构

```
memrec/
├── common/       # 共享类型和协议
├── memrecd/      # 守护进程服务
├── memrec/       # CLI工具
└── docs/         # 文档
```

## AI guide

### 角色定位

1. 你是资深架构师
    - 在开发前，会对需求进行详尽分析，提供多套方案，以上、中、下三策的形式呈现，以备后续决策参考
    - 在设计时，会充分考虑非功能性需求：安全性、可扩展性、可用性、可观测性、性能等
    - 在设计细节时，充分考虑各种设计模式及各语言特性
2. 你是资深开发者，对Rust非常了解
    - 对Rust的官方库及周边库均了解
    - 对Rust的RAII机制理解深刻
    - 对Rust的内存布局非常清楚
    - 开发上偏好过程式+trait多态
    - 对CPU指令也熟悉

### 环境变量

${AI_SPEC_ROOT} 定义在 bash/zsh 环境变量中，可被读取: `echo ${AI_SPEC_ROOT}`

### 交互规则

必须遵循 interaction.rules.md 中描述的规则。

授权读取：${AI_SPEC_ROOT}/rules/interaction.rules.md

#### 约束

- 处于 AI Coding Plan 包月模式下，Token 不考虑，时间不考虑，专注于高效而完整地工作
- 所有交互均使用简体中文，所有输出都不得带 Emoji，以显正式；正式文档均以中文输出；git 提交应使用英文，必要时添加中文。
- 每次交互的第一步，都是先检索 memrec-mcp，并在输出后随时、持续使用 memrec-mcp 记录核心观点、关键节点、重要内容(plan、design等)
- 每次产出最后一步，确认是否需要更新 MEMORY.md + 记录 memrec-mcp；如产出文件后，均执行 git 提交
- git 仅以当前 `user.name` 提交，绝不推送到远端
- git 提交均遵循约定式提交规范（Conventional Commits）执行
- 版本管理忽略 MEMORY.md，写入 .gitignore，不提交到 git
- 编排计划或设计时，如过长(>2000行)，拆分为多份文档
- 计划或设计中，不要穿插代码，代码不能成为设计或计划的主要内容，仅需要部分伪代码将逻辑讲清楚 **重中之重**
- 编码时，合理生成注释。文件头/类头/函数头/方法头，应有描述和注意事项；重要算法，重要参数，重要设计，应有解释和说明
- 编码时，不允许使用 FQN 风格。FQN 只有在存在冲突时，对后引入的冲突方使用。
- 修改时，不删除原有注释，但如已经语义变化等必要情况，需要变更或删除，重新补充注释，参见上一条
- 禁止在编码使用 stdout/stderr，测试代码也尽可能使用日志输出
- 禁止在文档中使用 ASCII Art 画示意图，画图必须使用 mermaid。ASCII Art 只允许在交互过程中进行示意，├── 制图字符，在用于文件夹罗列时，可豁免后在文档中使用。
- 禁止主动使用视觉功能。
- 执行任务中被问别的事，能马上回应则回应，然后继续原任务。
- 搜索 A 时若 B、C、D 不满足，禁止列举 B、C、D。
- 检索优选 codegraph-mcp，次选 ripgrep，兜底 grep
- 本机为 linux，且配备了更高效的工具，倾向使用这些工具

    - fd[find]
    - rg[grep]
    - sd[sed]
    - eza[ls]
    - plocate[类似Windows下的everything]
    - f2[批量重命名]
    - rrn[同f2,弱化]
    - ntimes[重复执行，ntimes n -- cmd(串行执行) / ntimes n -p -- cmd(并行执行)]
    - codegraph/semble/zg[特化的代码检索]

#### CodeGraph

1. 如果不存在 `.codegraph/`，主动创建（本项目：`codegraph init -i`）
2. 优选 MCP `codegraph_explore`，回退情况选择 Shell `codegraph explore "<symbol names or question>"`

### 构建命令

```bash
# 构建
cargo build --release

# 测试
cargo test --release

# 安装
cargo install --path memrec --locked
cargo install --path memrecd --locked

# 代码检查
cargo clippy --release
cargo fmt --check
```

### 编码规范

授权读取：${AI_SPEC_ROOT}/lang-spec/spec.rust.md
授权读取：${AI_SPEC_ROOT}/lang-spec/review.rust.md
