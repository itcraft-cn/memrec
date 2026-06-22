# MemRec Daemon — AI Memory Persistence Server

[![Crates.io](https://img.shields.io/crates/v/memrecd.svg)](https://crates.io/crates/memrecd)
[![Documentation](https://docs.rs/memrecd/badge.svg)](https://docs.rs/memrecd)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

The daemon server for MemRec AI memory persistence system, providing persistent storage, semantic search, and project isolation.

## Overview

`memrecd` is the core server component of the MemRec ecosystem. It runs as a background daemon, exposing a JSON-RPC 2.0 API over Unix socket for memory operations, semantic search, and project management.

## Features

- **Persistent Storage**: RocksDB-based storage for metadata and vector embeddings
- **Semantic Search**: Vector similarity search using ONNX models (MiniLM-L6-v2, BGE-M3)
- **Project Isolation**: Separate memory spaces per project with automatic detection
- **Unix Socket API**: JSON-RPC 2.0 interface for local communication
- **Importance Scoring**: Automatic importance calculation based on recency, access count, and relevance
- **Chunked Storage**: Support for large memories with automatic chunking
- **Embedding Generation**: Integration with fastembed for efficient embeddings

## Installation

### From crates.io (Recommended)

```bash
cargo install --locked memrecd
```

### Using mr-install (All-in-one)

```bash
cargo install --locked mr-install
mr-install
```

## Usage

### Starting the Daemon

```bash
# Start the daemon (will run in background)
memrecd

# Start with verbose logging
RUST_LOG=debug memrecd

# Check daemon status
systemctl --user status memrecd  # Linux
launchctl list com.itcraft.memrecd  # macOS
```

### Configuration

The daemon reads configuration from `~/.memrec/config.toml`:

```toml
version = "0.3.0"

[model]
model_type = "minilm-l6-v2"  # or "bge-m3"
source = "huggingface"
dimension = 384  # 1024 for BGE-M3

[server]
socket_path = "~/.memrec/memrecd.sock"
data_dir = "~/.memrec/data"
vector_dir = "~/.memrec/vectors"
log_path = "~/.memrec/memrecd.log"

# Model files with SHA256 hashes for security
[[model.files]]
filename = "model.onnx"
sha256 = "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"
required = true

[[model.files]]
filename = "tokenizer.json"
sha256 = "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0"
required = true
```

### Architecture

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Unix Socket   │────▶│   JSON-RPC 2.0  │────▶│  Request Router │
│    Interface    │     │     Handler     │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                                                            │
                                                            ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Vector Store   │◀───▶│  Embedding Gen  │◀───▶│   Model Config  │
│   (RocksDB)     │     │   (fastembed)   │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
        │                        │                        │
        ▼                        ▼                        ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Metadata Store │     │  Project Detect │     │   Importance    │
│   (RocksDB)     │     │   (.mr_pid)     │     │   Calculator    │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

## API Reference

### JSON-RPC Methods

The daemon supports the following JSON-RPC 2.0 methods:

#### Memory Operations
- `add_memory` - Add a new memory with optional tags and importance
- `get_memory` - Retrieve a memory by ID
- `update_memory` - Update an existing memory
- `delete_memory` - Soft delete a memory
- `list_memories` - List memories with pagination
- `search_memories` - Semantic search with relevance scoring

#### Project Operations
- `get_project_info` - Get current project information
- `set_project` - Manually set project context
- `list_projects` - List all projects

#### System Operations
- `ping` - Health check
- `stats` - Get server statistics
- `version` - Get server version

### Example API Usage

```bash
# Using curl to interact with the socket
echo '{
  "jsonrpc": "2.0",
  "id": "1",
  "method": "add_memory",
  "params": {
    "id": "test-123",
    "content": "This is a test memory",
    "mtype": "conversation",
    "tags": ["test", "example"],
    "importance": 0.8
  }
}' | socat UNIX-CONNECT:$HOME/.memrec/memrecd.sock STDIO
```

## Development

### Building from Source

```bash
# Clone repository
git clone https://github.com/itcraft-cn/memrec
cd memrec

# Build in release mode
cargo build --release --bin memrecd

# Run tests
cargo test --release --bin memrecd
```

### Running Tests

```bash
# Run all tests
cargo test --release

# Run specific test categories
cargo test --release --test embedding
cargo test --release --test storage
cargo test --release --test server
```

### Logging

The daemon uses `tracing` for structured logging:

```bash
# Different log levels
RUST_LOG=error memrecd     # Only errors
RUST_LOG=warn memrecd      # Warnings and errors
RUST_LOG=info memrecd      # Info level (default)
RUST_LOG=debug memrecd     # Debug information
RUST_LOG=trace memrecd     # Verbose tracing
```

## Performance

### Memory Usage
- **Metadata**: ~50 bytes per memory entry
- **Vectors**: 384 bytes (MiniLM-L6-v2) or 1024 bytes (BGE-M3) per memory
- **Index**: Additional ~20% overhead for vector indices

### Throughput
- **Embedding**: ~1000 texts/second on CPU
- **Search**: ~10,000 vectors/second for nearest neighbor search
- **Storage**: ~10,000 writes/second for metadata

### Scalability
- Supports millions of memories per project
- Automatic memory chunking for large contents
- Background importance recalculation

## Security

### Data Protection
- Project isolation prevents cross-project data access
- Unix socket permissions restrict access to owner
- Configuration files are user-mode only (600 permissions)

### Model Security
- SHA256 hash verification for downloaded models
- Optional `--skip-hash-verify` flag with security warnings
- Support for trusted mirrors with hash validation

### Service Security
- Runs as user service (not root)
- No network exposure by default
- Hardened service configuration files

## Troubleshooting

### Common Issues

1. **Socket Connection Failed**
   ```bash
   # Check if daemon is running
   ps aux | grep memrecd
   
   # Check socket permissions
   ls -la ~/.memrec/memrecd.sock
   
   # Restart daemon
   systemctl --user restart memrecd
   ```

2. **Model Download Failed**
   ```bash
   # Check network connectivity
   curl -I https://huggingface.co
   
   # Use mirror
   mr-install --use-hf-mirror
   
   # Skip hash verification (security risk)
   mr-install --skip-hash-verify
   ```

3. **Storage Issues**
   ```bash
   # Check disk space
   df -h ~/.memrec
   
   # Repair database
   rm -rf ~/.memrec/data
   rm -rf ~/.memrec/vectors
   # Re-run mr-install to recreate
   ```

### Logs
- Service logs: `~/.memrec/memrecd.log`
- System logs: `journalctl --user -u memrecd` (Linux)
- Launchd logs: `log stream --predicate 'subsystem == "com.itcraft.memrecd"'` (macOS)

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for development guidelines.

## License

Apache License 2.0 - see [LICENSE](../LICENSE) for details.

## Links

- [Main Repository](https://github.com/itcraft-cn/memrec)
- [API Documentation](https://docs.rs/memrecd)
- [Crates.io](https://crates.io/crates/memrecd)
- [CLI Client](../memrec/README.md)
- [Installer](../mr-install/README.md)