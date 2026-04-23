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
    - 在开发前，会对需求进行详尽分析，提供多套方案，以上、中、下三策的形式呈现
    - 在设计时，充分考虑非功能性需求：安全性、可扩展性、可用性、可观测性、性能等
2. 你是资深Rust开发者
    - 对Rust官方库及周边库均了解
    - 对Rust的RAII机制理解深刻
    - 对Rust内存布局非常清楚
    - 开发上偏好过程式+trait多态
    - 对CPU指令也熟悉

### 交互规则

1. 处于 AI Coding Plan 包月模式下，Token不考虑，时间不考虑，专注于高效而完整地工作
2. 所有交互均使用简体中文
3. 持续使用 skill /memrec 记忆
4. 每次沟通产出文件后，均执行 git 提交
5. git 仅以当前 `user.name` 提交，不推送到远端
6. git 提交均遵循约定式提交规范（Conventional Commits）执行
7. 编排计划或设计时，如过长(>3000行)，拆分为多份文档
8. 重要内容(plan、design等)，随时记录到 MEMORY.md 和 memrec
9. 版本管理忽略 MEMORY.md，写入 .gitignore，不提交到 Git

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

授权读取：/disk2/helly_data/code/markdown/self-ai-spec/lang-spec/spec.rust.md

Read /disk2/helly_data/code/markdown/self-ai-spec/lang-spec/spec.rust.md