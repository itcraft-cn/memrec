# MemRec Common — 共享类型与协议

[![Crates.io](https://img.shields.io/crates/v/memrec-common.svg)](https://crates.io/crates/memrec-common)
[![文档](https://docs.rs/memrec-common/badge.svg)](https://docs.rs/memrec-common)
[![许可证](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

MemRec AI 记忆持久化系统的共享类型、协议和工具库。

## 概述

`memrec-common` 为 MemRec 生态系统提供了基础数据结构与通信协议。该 crate 确保 CLI 客户端、守护进程服务器和安装器组件之间的一致性。

## 特性

- **核心类型**: `Memory`、`Project`、`ImportanceConfig` 等基础数据结构
- **JSON-RPC 2.0 协议**: 客户端-服务器通信的请求/响应类型
- **序列化**: 所有数据结构完整的 serde 支持
- **零拷贝**: 高性能应用的高效内存处理
- **跨组件兼容性**: 保证所有 MemRec 组件之间的一致性

## 安装

添加到 `Cargo.toml`:

```toml
[dependencies]
memrec-common = "0.3.0"
```

## 使用示例

### 核心类型

```rust
use memrec_common::types::{Memory, MemoryType, Project};

// 创建新记忆
let memory = Memory::new(
    "conversation-123",
    "项目规划讨论",
    MemoryType::Conversation,
    vec!["会议", "规划"],
    "2024-01-15T10:30:00+08:00",
    0.85
);

// 创建项目
let project = Project::new(
    "my-project",
    "个人知识库",
    vec!["rust", "ai", "memory"]
);
```

### 协议使用

```rust
use memrec_common::protocol::{
    JsonRpcRequest, JsonRpcResponse,
    MemoryRequest, MemoryResponse
};

// 创建添加记忆的请求
let request = JsonRpcRequest::Memory(MemoryRequest::Add {
    id: "test-memory".to_string(),
    content: "测试内容".to_string(),
    mtype: "conversation".to_string(),
    tags: Some(vec!["test".to_string()]),
    importance: Some(0.5),
});

// 解析响应
let response_json = r#"{
    "jsonrpc": "2.0",
    "id": "1",
    "result": {
        "success": true,
        "memory_id": "test-memory"
    }
}"#;

let response: JsonRpcResponse = serde_json::from_str(response_json)?;
```

### 配置

```rust
use memrec_common::types::config::{MemoryConfig, ImportanceConfig};

// 记忆配置
let mem_config = MemoryConfig {
    max_memories_per_project: 1000,
    auto_cleanup_days: 30,
    importance_decay_factor: 0.95,
};

// 重要性计算配置
let imp_config = ImportanceConfig {
    recent_days_weight: 0.4,
    access_count_weight: 0.3,
    tag_similarity_weight: 0.3,
};
```

## API 参考

### 核心模块

- **`types`**: 基础数据结构
  - `Memory` - 带有元数据的单个记忆条目
  - `Project` - 项目隔离与配置
  - `MemoryConfig` - 记忆存储配置
  - `ImportanceConfig` - 重要性计算参数

- **`protocol`**: JSON-RPC 2.0 通信
  - `JsonRpcRequest` - 请求类型（添加、获取、搜索等）
  - `JsonRpcResponse` - 带有成功/错误处理的响应类型
  - `SemanticSearchParams` - 语义搜索参数

- **`error`**: 错误类型和处理
  - `MemRecError` - 所有操作的统一错误类型
  - `JsonRpcError` - JSON-RPC 特定的错误处理

### 序列化

所有类型都实现了 `serde::Serialize` 和 `serde::Deserialize`，具有合理的默认值：

```rust
use memrec_common::types::Memory;
use serde_json;

let memory = Memory::new(/* ... */);
let json = serde_json::to_string_pretty(&memory)?;
let deserialized: Memory = serde_json::from_str(&json)?;
```

## 特性标志

- `default`: 包含所有核心功能
- `full`: 添加额外的工具和辅助函数（默认启用）

## 集成

该 crate 设计用于：

1. **`memrec` CLI**: 客户端类型定义和协议处理
2. **`memrecd` 守护进程**: 服务器端类型匹配用于 RPC 通信
3. **`mr-install`**: 配置和设置类型

## 开发

### 构建

```bash
cargo build --release
```

### 测试

```bash
cargo test --release
```

### 文档

```bash
cargo doc --open
```

## 版本控制

遵循[语义化版本控制](https://semver.org/)。主版本变更表示破坏性 API 更改。

## 许可证

Apache 许可证 2.0 - 详见 [LICENSE](../LICENSE)。

## 链接

- [主仓库](https://github.com/itcraft-cn/memrec)
- [API 文档](https://docs.rs/memrec-common)
- [Crates.io](https://crates.io/crates/memrec-common)