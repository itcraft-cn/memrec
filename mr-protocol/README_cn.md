# mr-protocol

MemRec 生态系统的 JSON-RPC 2.0 协议类型库。

## 概述

`mr-protocol` 为 `memrec` CLI 与 `memrecd` 守护进程之间的 JSON-RPC 2.0 通信提供类型定义。实现了完整的请求/响应协议，具备强类型和 serde 序列化支持。

## 特性

- **JSON-RPC 2.0 标准**: 遵循 JSON-RPC 2.0 规范
- **类型安全**: 所有请求参数和响应结果均为强类型
- **Serde 集成**: 基于 serde 的自动序列化/反序列化
- **完整 API**: 支持记忆 CRUD、语义搜索、项目管理、配置等操作

## 使用方法

添加到 `Cargo.toml`:

```toml
[dependencies]
mr-protocol = { path = "../mr-protocol" }
```

### 请求示例

```rust
use mr_protocol::{JsonRpcRequest, RequestAction, AddParams};

let request = JsonRpcRequest {
    jsonrpc: "2.0".to_string(),
    id: 1,
    method: RequestAction::Add,
    params: RequestParams::Add(AddParams {
        content: "重要决策: 后端使用 Rust 开发".to_string(),
        memory_type: MemoryType::Decision,
        tags: vec!["backend".to_string(), "rust".to_string()],
        is_global: false,
    }),
};
```

### 响应示例

```rust
use mr_protocol::{JsonRpcResponse, MemoryResult, ResponseResult};

let response = JsonRpcResponse::success(
    ResponseResult::Memory(MemoryResult {
        id: uuid::Uuid::new_v4(),
        success: true,
        message: "记忆添加成功".to_string(),
    }),
    1,
);
```

## API 参考

### 请求类型

| 操作 | 说明 | 参数类型 |
|------|------|----------|
| `Add` | 添加新记忆 | `AddParams` |
| `Get` | 按 ID 获取记忆 | `GetParams` |
| `Update` | 更新已有记忆 | `UpdateParams` |
| `Delete` | 按 ID 删除记忆 | `DeleteParams` |
| `Search` | 语义搜索 | `SearchMemoryParams` |
| `List` | 列出记忆 | `ListParams` |
| `Dream` | 压缩旧记忆 | `DreamParams` |
| `GetProjectInfo` | 获取当前项目信息 | `GetProjectInfoParams` |
| `GetVersion` | 获取守护进程版本 | - |
| `Stats` | 获取记忆统计 | - |

### 响应类型

| 结果类型 | 说明 |
|----------|------|
| `MemoryResult` | 单条记忆操作结果 |
| `MemoryListResult` | 记忆列表 |
| `SemanticSearchResult` | 搜索命中结果 |
| `StatsResult` | 记忆统计信息 |
| `ProjectInfoResult` | 项目信息 |
| `VersionResult` | 守护进程版本信息 |
| `SuccessResult` | 通用成功/失败结果 |

## 错误处理

所有错误遵循 JSON-RPC 2.0 错误格式:

```rust
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}
```

标准错误码:
- `-32700`: 解析错误
- `-32600`: 无效请求
- `-32601`: 方法不存在
- `-32602`: 无效参数
- `-32603`: 内部错误
- 自定义错误码: 应用特定错误

## 协议流程

```
客户端 (memrec)                守护进程 (memrecd)
     |                              |
     |  JsonRpcRequest              |
     |----------------------------->|
     |                              | 处理请求
     |                              |
     |  JsonRpcResponse             |
     |<-----------------------------|
     |                              |
```

## 许可证

Apache-2.0
