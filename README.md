# MemRec - AI CLI 记忆持久化系统

本地化的记忆持久化系统，为 AI CLI 工具提供跨会话记忆恢复、知识库积累、对话历史存档能力。

## 项目结构

```
memrec/
├── common/       # 共享类型和协议定义
├── memrecd/      # 守护进程服务
├── memrec/       # CLI 工具
└── docs/         # 设计文档和计划
```

## 构建状态

Phase 1 (基础设施): ✅ 完成
- Workspace 结构
- Memory/Project/Config 类型
- JSON-RPC 协议

## 构建

```bash
cargo build
cargo test
```

## 文档

- [设计文档](docs/superpowers/specs/2026-04-23-memrec-design.md)
- [算法文档](docs/superpowers/specs/2026-04-23-memrec-algorithms.md)
- [实现计划](docs/superpowers/plans/)