# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0-dev] - 2026-04-23

### Added

#### Phase 6: Semantic Search & Project Isolation
- Real semantic embeddings with FastEmbed (all-MiniLM-L6-v2)
- Local ONNX model loading from ~/.memrec/models/
- Configurable model path via MEMREC_MODEL_DIR env var
- RocksDB-based vector persistence (vectors column family)
- Auto-rebuild missing embeddings on startup
- 30s sync interval + graceful shutdown save
- Project memory isolation with .mr_pid file
- Public vs project memory separation
- Semantic search with meaningful similarity scores

#### Search Enhancements
- `memrec search "query"` - positional argument syntax
- --min-score default changed to 0.0 (hash embedding had low scores)
- --project-only and --global-only filters
- --human output format

#### Buffer Overflow Fix
- Dynamic response buffer (1MB limit)
- Server shutdown after sending response

### Changed

- Vector storage: RocksDB instead of in-memory
- Embedding: Real ONNX model instead of hash placeholder
- Search syntax: removed redundant --query flag
- Default model path: ~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx/

### Technical Details

- Model: all-MiniLM-L6-v2 (384 dimensions, ~90MB)
- Vector storage: ~/.memrec/data/vectors/
- Data directory structure updated
- Performance: ~130MB memory with model loaded

---

## [0.1.0] - 2026-04-23

### Added

#### Core Features
- Memory persistence system with RocksDB storage backend
- Unix Socket daemon (memrecd) with JSON-RPC 2.0 protocol
- CLI tool (memrec) for memory management
- 5 memory types: decision, knowledge, context, preference, conversation
- Tag-based categorization with importance weighting
- Hybrid search: exact + semantic retrieval with RRF fusion algorithm
- Automatic long content splitting (>7.5KB with warnings)
- UTF-8 safe character boundary handling

#### Storage Layer
- RocksDB integration with 7 Column Families
- In-memory vector store with cosine similarity
- Memory, Project, Config, Vector storage traits
- Soft delete and hard delete lifecycle
- Importance-based memory indexing

#### Server Layer
- Unix Socket server with async handling
- JSON-RPC Router with action handlers
- Add, Get, List, Delete, Stats handlers
- Signal handling (SIGTERM, SIGINT)
- Automatic socket cleanup on shutdown

#### Lifecycle Management
- Importance calculator with decay algorithm
- Time decay: exponential with λ=0.05
- Frequency factor: logarithmic growth
- Semantic importance: tag-based weights (critical=1.0, draft=0.1)
- Lifecycle manager with cleanup cycle
- Automatic memory compression and forgetting

#### Integration
- Systemd user service with install scripts
- Skill for AI CLI tools (opencode, claude code)
- Convenient management script (memrecctl.sh)
- Status checking script
- Uninstall script with data cleanup option

#### Documentation
- System design specification
- Algorithms and strategies document
- 5-phase implementation plan
- Systemd guide
- Skill documentation
- Chinese and English README

### Technical Details

#### Architecture
- Rust workspace with 3 crates: common, memrecd, memrec
- Async runtime: Tokio
- Storage: RocksDB + in-memory vector store
- Communication: Unix Socket + JSON-RPC
- CLI: clap with subcommands

#### Performance
- Unix Socket communication: 8KB buffer
- Auto-splitting threshold: 7.5KB
- Memory footprint: ~3.2MB for daemon
- Storage overhead: ~2KB per memory

#### Testing
- 34 unit tests across all modules
- Common crate: 20 tests
- Memrecd crate: 13 tests  
- Memrec crate: 1 test
- Integration tests: daemon lifecycle, CLI commands

### Known Limitations

- Qdrant client library doesn't support embedded mode (placeholder kept)
- Model must be downloaded manually (~90MB)

### Breaking Changes

None - initial release

### Migration Guide

None - initial release

### Contributors

- Initial implementation by MemRec Team

### Future Plans

#### Phase 2 (HTTP API)
- HTTP REST API endpoint
- Web management interface
- OpenAPI specification
- Authentication support

#### Phase 3 (MCP Protocol)
- MCP server integration
- Tool definitions for AI agents
- Resource templates

#### Phase 4 (Embedding)
- Local embedding model (candle-transformers)
- Embedding cache with RocksDB
- Semantic search enhancement

#### Phase 5 (Advanced Features)
- Memory clustering and deduplication
- Timeline visualization
- Export/Import enhancements
- Memory graph visualization

---

## Version History Summary

| Version | Date | Highlights |
|---------|------|------------|
| 0.2.0-dev | 2026-04-23 | Semantic search, project isolation, real embeddings |
| 0.1.0 | 2026-04-23 | Initial release with core features |

---

## Development Milestones

### Phase 6: Semantic Search (Completed)
- Real FastEmbed embeddings
- RocksDB vector persistence
- Project isolation
- 51 tests passing

### Phase 1: Infrastructure (Completed)
- Workspace structure
- Memory/Project/Config types
- JSON-RPC protocol types
- 20 tests passing

### Phase 2: Storage Layer (Completed)
- RocksDB integration
- MemoryStore implementation
- VectorStore (in-memory)
- 9 tests passing

### Phase 3: Server Layer (Completed)
- Unix Socket server
- JSON-RPC Router
- Daemon management
- 10 tests passing

### Phase 4: CLI Tool (Completed)
- Unix Socket client
- Memory commands
- Stats command
- 1 test passing

### Phase 5: Advanced Features (Completed)
- Importance calculator
- Lifecycle manager
- 2 tests passing

### Post-Release (Completed)
- UTF-8 boundary fix
- Auto-split long content
- Systemd integration
- Skill creation
- Chinese README

---

## Release Checklist

- [x] All tests passing (51 tests)
- [x] Documentation complete
- [x] Systemd service tested
- [x] CLI commands verified
- [x] Skill integration tested
- [x] UTF-8 handling fixed
- [x] Long content splitting
- [x] CHANGELOG created
- [x] Semantic search working
- [x] Model download documented

---

## Links

- [Design Document](docs/superpowers/specs/2026-04-23-memrec-design.md)
- [Algorithms Document](docs/superpowers/specs/2026-04-23-memrec-algorithms.md)
- [Implementation Plans](docs/superpowers/plans/)
- [Skill Documentation](docs/skills/memrec-skill.md)