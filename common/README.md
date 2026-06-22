# MemRec Common — Shared Types and Protocols

[![Crates.io](https://img.shields.io/crates/v/memrec-common.svg)](https://crates.io/crates/memrec-common)
[![Documentation](https://docs.rs/memrec-common/badge.svg)](https://docs.rs/memrec-common)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Shared types, protocols, and utilities for the MemRec AI memory persistence system.

## Overview

`memrec-common` provides the foundational data structures and communication protocols used across the MemRec ecosystem. This crate ensures consistency between the CLI client, daemon server, and installer components.

## Features

- **Core Types**: `Memory`, `Project`, `ImportanceConfig`, and other fundamental data structures
- **JSON-RPC 2.0 Protocol**: Request/response types for client-server communication
- **Serialization**: Complete serde support for all data structures
- **Zero-copy**: Efficient memory handling for high-performance applications
- **Cross-component Compatibility**: Guaranteed consistency between all MemRec components

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
memrec-common = "0.3.0"
```

## Usage Examples

### Core Types

```rust
use memrec_common::types::{Memory, MemoryType, Project};

// Create a new memory
let memory = Memory::new(
    "conversation-123",
    "Project planning discussion",
    MemoryType::Conversation,
    vec!["meeting", "planning"],
    "2024-01-15T10:30:00Z",
    0.85
);

// Create a project
let project = Project::new(
    "my-project",
    "Personal knowledge base",
    vec!["rust", "ai", "memory"]
);
```

### Protocol Usage

```rust
use memrec_common::protocol::{
    JsonRpcRequest, JsonRpcResponse,
    MemoryRequest, MemoryResponse
};

// Create a request to add memory
let request = JsonRpcRequest::Memory(MemoryRequest::Add {
    id: "test-memory".to_string(),
    content: "Test content".to_string(),
    mtype: "conversation".to_string(),
    tags: Some(vec!["test".to_string()]),
    importance: Some(0.5),
});

// Parse a response
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

### Configuration

```rust
use memrec_common::types::config::{MemoryConfig, ImportanceConfig};

// Memory configuration
let mem_config = MemoryConfig {
    max_memories_per_project: 1000,
    auto_cleanup_days: 30,
    importance_decay_factor: 0.95,
};

// Importance calculation configuration
let imp_config = ImportanceConfig {
    recent_days_weight: 0.4,
    access_count_weight: 0.3,
    tag_similarity_weight: 0.3,
};
```

## API Reference

### Core Modules

- **`types`**: Fundamental data structures
  - `Memory` - Individual memory entries with metadata
  - `Project` - Project isolation and configuration
  - `MemoryConfig` - Memory storage configuration
  - `ImportanceConfig` - Importance calculation parameters

- **`protocol`**: JSON-RPC 2.0 communication
  - `JsonRpcRequest` - Request types (add, get, search, etc.)
  - `JsonRpcResponse` - Response types with success/error handling
  - `SemanticSearchParams` - Parameters for semantic search

- **`error`**: Error types and handling
  - `MemRecError` - Unified error type for all operations
  - `JsonRpcError` - JSON-RPC specific error handling

### Serialization

All types implement `serde::Serialize` and `serde::Deserialize` with sensible defaults:

```rust
use memrec_common::types::Memory;
use serde_json;

let memory = Memory::new(/* ... */);
let json = serde_json::to_string_pretty(&memory)?;
let deserialized: Memory = serde_json::from_str(&json)?;
```

## Feature Flags

- `default`: Includes all core functionality
- `full`: Adds additional utilities and helpers (enabled by default)

## Integration

This crate is designed to be used by:

1. **`memrec` CLI**: Client-side type definitions and protocol handling
2. **`memrecd` Daemon**: Server-side type matching for RPC communication
3. **`mr-install`**: Configuration and setup types

## Development

### Building

```bash
cargo build --release
```

### Testing

```bash
cargo test --release
```

### Documentation

```bash
cargo doc --open
```

## Versioning

Follows [Semantic Versioning](https://semver.org/). Major version changes indicate breaking API changes.

## License

Apache License 2.0 - see [LICENSE](../LICENSE) for details.

## Links

- [Main Repository](https://github.com/itcraft-cn/memrec)
- [API Documentation](https://docs.rs/memrec-common)
- [Crates.io](https://crates.io/crates/memrec-common)