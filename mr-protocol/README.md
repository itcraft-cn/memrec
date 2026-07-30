# mr-protocol

JSON-RPC 2.0 protocol types for MemRec ecosystem.

## Overview

`mr-protocol` provides type definitions for JSON-RPC 2.0 communication between `memrec` CLI and `memrecd` daemon. It implements the complete request/response protocol with strong typing and serde serialization.

## Features

- **JSON-RPC 2.0 Compliant**: Follows the JSON-RPC 2.0 specification
- **Type-Safe**: All request parameters and response results are strongly typed
- **Serde Integration**: Automatic serialization/deserialization with serde
- **Comprehensive API**: Supports memory CRUD, semantic search, project management, configuration, and more

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
mr-protocol = { path = "../mr-protocol" }
```

### Request Example

```rust
use mr_protocol::{JsonRpcRequest, RequestAction, AddParams};

let request = JsonRpcRequest {
    jsonrpc: "2.0".to_string(),
    id: 1,
    method: RequestAction::Add,
    params: RequestParams::Add(AddParams {
        content: "Important decision: Use Rust for the backend".to_string(),
        memory_type: MemoryType::Decision,
        tags: vec!["backend".to_string(), "rust".to_string()],
        is_global: false,
    }),
};
```

### Response Example

```rust
use mr_protocol::{JsonRpcResponse, MemoryResult, ResponseResult};

let response = JsonRpcResponse::success(
    ResponseResult::Memory(MemoryResult {
        id: uuid::Uuid::new_v4(),
        success: true,
        message: "Memory added successfully".to_string(),
    }),
    1,
);
```

## API Reference

### Request Types

| Action | Description | Params Type |
|--------|-------------|-------------|
| `Add` | Add a new memory | `AddParams` |
| `Get` | Retrieve memory by ID | `GetParams` |
| `Update` | Update existing memory | `UpdateParams` |
| `Delete` | Delete memory by ID | `DeleteParams` |
| `Search` | Semantic search | `SearchMemoryParams` |
| `List` | List memories | `ListParams` |
| `Dream` | Consolidate old memories | `DreamParams` |
| `GetProjectInfo` | Get current project info | `GetProjectInfoParams` |
| `GetVersion` | Get daemon version | - |
| `Stats` | Get memory statistics | - |

### Response Types

| Result Type | Description |
|-------------|-------------|
| `MemoryResult` | Single memory operation result |
| `MemoryListResult` | List of memories |
| `SemanticSearchResult` | Search results with hits |
| `StatsResult` | Memory statistics |
| `ProjectInfoResult` | Project information |
| `VersionResult` | Daemon version info |
| `SuccessResult` | Generic success/failure |

## Error Handling

All errors follow the JSON-RPC 2.0 error format:

```rust
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}
```

Standard error codes:
- `-32700`: Parse error
- `-32600`: Invalid request
- `-32601`: Method not found
- `-32602`: Invalid params
- `-32603`: Internal error
- Custom codes: Application-specific errors

## Protocol Flow

```
Client (memrec)                Daemon (memrecd)
     |                              |
     |  JsonRpcRequest              |
     |----------------------------->|
     |                              | Process request
     |                              |
     |  JsonRpcResponse             |
     |<-----------------------------|
     |                              |
```

## License

Apache-2.0
