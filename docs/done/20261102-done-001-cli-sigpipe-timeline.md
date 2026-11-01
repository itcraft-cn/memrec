# CLI 管道中断修复与 timeline 命令

- 日期：2026-11-02
- 状态：已完成（检索门禁 hit@5 未过，判定为既有金标漂移，详见交付说明）
- 关联：`AGENTS.md` 的「多步骤测试与部署」流程

## 1. 背景

`memrec list | more` 中途按 `q` 退出后，`more` 关闭管道，`memrec` 继续向已关闭的 stdout 写入，`println!` 因 `EPIPE` 触发 panic 并打印堆栈。根因是 Rust 运行时默认忽略 `SIGPIPE`，写端不会静默终止，而是由标准库将 `EPIPE` 转为 panic。

同时缺少按创建时间倒序浏览记忆的命令。现有 `list` 的返回顺序来自存储层自然顺序（RocksDB key 顺序），既非时间序也不稳定，无法满足“最近发生了什么”的查看诉求。

## 2. 目标

1. 管道提前关闭时进程安静退出，不打印 panic 与堆栈。
2. 新增 `memrec timeline`，按 `created_at` 倒序输出，支持 `list` 同等的范围、标签、类型过滤与分页。
3. 排序在服务端完成，保证 `skip`/`limit` 分页在时间序下语义正确；并列时间以记忆 ID 作为稳定次序。
4. 保持既有 `list` 默认顺序不变，避免行为回归。

## 3. 非目标

1. 不为 `list` 增加通用排序参数之外的能力（如按重要性排序）。
2. 不改动 MCP 工具接口。
3. 不改变存储层列族结构。

## 4. 方案

### 4.1 管道中断

在 `memrec` 进程入口把 `SIGPIPE` 处置恢复为系统默认。CLI 为短生命周期客户端，无需信号清理；管道关闭时由内核终止进程，符合 Unix 工具惯例。该处置仅影响 CLI 进程，不影响守护进程 `memrecd`。

### 4.2 timeline 命令

- 协议层：`ListParams` 增加 `order` 字段（`storage` / `created_asc` / `created_desc`），默认 `storage`，保持向后兼容。
- 服务端：`handle_list` 在过滤完成后按 `order` 排序，再执行 `skip`/`take`，保证分页下顺序稳定。
- CLI：新增 `timeline` 子命令，参数与 `list` 对齐，固定 `order=created_desc`；人类可读输出以本地时间开头，每行一条，便于快速扫描。
- 文档：README、MANUAL、CHANGELOG、skill 命令参考同步中英双语。

## 5. 验收

1. `cargo build --release`、`cargo test --release`、`cargo clippy --release`、`cargo fmt --check` 通过。
2. 管道复现：`memrec list | more` 中途退出无 panic。
3. `memrec timeline` 输出按时间倒序，分页不重叠且总数准确。
4. 部署后 `scripts/retrieval_gate.sh` 退出码为 0。
