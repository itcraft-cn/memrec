# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-07-16

### Added

- **Hybrid Search**: KNN + BM25 full-text search with configurable `--hybrid-alpha` (default 0.5)
- **MMR Reranking**: Maximal Marginal Relevance for diverse results, `--mmr-enabled` (default true), `--mmr-lambda` (default 0.7)
- **Time Decay Scoring**: Recent memories ranked higher, configurable decay rate
- **Evergreen Exemption**: Knowledge/decision types exempt from time decay
- **Source Weighting**: User memories weighted higher than system/inferred
- **Chinese Text Search**: N-gram tokenizer (2-4 grams) in Tantivy for Chinese support
- `MemorySource` enum: `User`, `System`, `Inferred`, `External`
- `MemoryScope` enum: `Project`, `Global`, `Workspace`
- `--source` and `--scope` flags on `memrec add`
- `SearchConfig`, `SourceWeights` configuration types
- `HybridStorage` trait and `HybridStore` implementation
- `FtsStorage` trait and `TantivyStore` (Tantivy 0.22) for BM25 search

### Changed

- Search pipeline: KNN+BM25 parallel → merge normalize → time decay + source weight → MMR rerank
- Search results include `created_at` field
- `memrecd` binary size: 42MB → 47MB (Tantivy dependency)
- QueryParser requires `IndexRecordOption::WithFreqsAndPositions` for n-gram tokenizer

### Fixed

- `source`/`scope` params not passed from AddParams to Memory entity
- Tantivy QueryParser error: "field does not have positions indexed"

### Breaking Changes

- Tantivy schema changed: delete `~/.memrec/fts/` before upgrade, index will rebuild automatically

---

## [0.4.0] - 2026-07-16

### Added

- Multi-model embedding support: choose MiniLM-L6-v2 (384d, English) or BGE-M3 (1024d, multilingual/Chinese) at install time
- `--model` flag on `mr-install`: `--model bge-m3` or `--model minilm-l6-v2` (default)
- `ModelType`, `ModelConfig`, `ModelFile`, `ModelFileType`, `PoolingStrategy` types in common
- `FastEmbedGenerator` unified with `UserDefinedEmbedding` + external initializer + configurable pooling (Cls for BGE-M3, Mean for MiniLM)
- Model-driven download: `mr-install` reads `ModelConfig` to determine which files to download, with SHA-256 verification
- `--skip-hash-verify` and `--allow-any-repo` flags on `mr-install` for advanced use
- `default_min_score()` on `ModelType`: 0.75 for MiniLM, 0.5 for BGE-M3 (BGE-M3 cosine scores are inherently lower)
- New config.toml format: `[model]` section with `model_type`, `source`, `dimension`, `[[model.files]]` array
- Nested `DaemonServerConfig` in `DaemonConfig` with `expand_tilde` for path resolution
- `sentencepiece.bpe.model` support in `ModelFileType` for BGE-M3 tokenizer
- Comprehensive user manual: `MANUAL.md` (English) and `MANUAL_cn.md` (Chinese)

### Changed

- Default min_score: 0.75 → 0.5 (BGE-M3 produces lower cosine similarity; MiniLM still uses 0.75 via `ModelType::default_min_score()`)
- Config format: flat `name = "Qdrant/all-MiniLM-L6-v2-onnx"` → structured `[model]` section with `model_type` and `[[model.files]]`
- `mr-install` now generates config based on selected model type
- Vector dimension: 384 (MiniLM) or 1024 (BGE-M3), determined by model selection
- Memory usage: ~118MB (MiniLM) / ~1.5GB (BGE-M3) at runtime
- Model download size: ~90MB (MiniLM) / ~2.3GB (BGE-M3)
- Documentation consolidated: `docs/user-guide.md` and `docs/installation.md` merged into `MANUAL.md` / `MANUAL_cn.md`
- README documentation links updated to point to MANUAL files

### Fixed

- Search returning 0 results with BGE-M3: adjusted default min_score from 0.75 to 0.5
- External ONNX data loading: `with_external_initializer()` for BGE-M3's `model.onnx_data` and `Constant_7_attr__value`
- BGE-M3 pooling: CLS pooling instead of Mean for correct embeddings

### Removed

- Old flat config format (`name = "Qdrant/all-MiniLM-L6-v2-onnx"` in config.toml)
- Hardcoded model path assumptions
- `docs/user-guide.md` (superseded by `MANUAL_cn.md`)
- `docs/installation.md` (superseded by `MANUAL.md` / `MANUAL_cn.md`)

---

## [0.3.0] - 2026-06-23

### Added

- `mr-install` crate: one-stop installer with `cargo install`, model download, service registration, and verification
- `ServiceManager` trait with Linux (systemd) and macOS (launchd) implementations
- Platform-specific default bin directories (`~/.local/bin/` on Linux, `~/bin/` on macOS)
- Install from crates.io by default, `--repo-url` for git source
- Security hardening for `mr-install`: SHA-256 model verification, repo URL whitelist, `--allow-any-repo` override
- `--version` flag on `memrecd`
- Comprehensive README documents for all subprojects (common, memrec, memrecd, mr-install)
- Security analysis documentation: `SECURITY_ANALYSIS.md`, `SECURITY_HIGHLIGHTS.md`, `SECURITY_SUMMARY.md`

### Changed

- Installation method: `install.sh` → `mr-install` (Rust-based, cross-platform)
- `docs/installation.md` updated for `mr-install` workflow

### Fixed

- `memrec get --merge` on non-chunked memory returns content directly instead of error
- Server handler improvements

### Removed

- Windows support (removed due to rocksdb bindgen/clang requirement)
- `install.sh` (replaced by `mr-install`)
- `docs/systemd.md` (consolidated into `docs/installation.md`)

---

## [0.2.0] - 2026-05-14

### Added

- MCP Server: `memrec --mcp` stdio mode, AI clients call directly via MCP protocol
- 6 MCP Tools: mr_add, mr_search, mr_get, mr_list, mr_delete, mr_stats
- 2 MCP Resources: memrec://stats, memrec://project
- Project isolation: client passes working_dir (git root preferred), server detects project_id from .mr_pid
- Cross-project search: `memrec search "query" --all` searches across all projects
- `--all` flag on search command
- `cross_project` field in SearchMemoryParams
- `working_dir` field in AddParams, SearchMemoryParams, GetProjectInfoParams
- Parameterized min_score default: `MEMREC_MIN_SCORE` env var (default 0.75)
- `default_min_score` and `default_include_global` exported from common lib
- Install script: `install.sh` (build, install, model download, systemd, test)
- Installation guide: `docs/installation.md`
- User guide: `docs/user-guide.md`
- Apache License 2.0

### Changed

- min_score default: 0.0 → 0.75 (filter low-relevance noise)
- Vector storage directory: `~/.memrec/data/vectors/` → `~/.memrec/vectors/` (separate from metadata)
- Knowledge type now supports tag-based subcategories: fact, best-practice, algorithm, tool
- SearchArgs fields made public for main.rs access
- README updated: project isolation, semantic search, cross-project search, MCP, data location

### Fixed

- Project isolation: client now passes working_dir, server uses it for .mr_pid detection (was broken - all memories wrote to same project_id)
- Buffer overflow: dynamic response buffer (1MB limit), server shutdown after sending
- RocksDB lock: test uses drop scope before reopening
- Clippy: 0 warnings

### Removed

- qdrant-client dependency (unused placeholder)
- qdrant.rs and persistent_vector_store.rs (unused)
- memrec-skill-phase6.md (outdated draft)

---

## [0.1.0] - 2026-04-23

### Added

- Memory persistence system with RocksDB storage backend
- Unix Socket daemon (memrecd) with JSON-RPC 2.0 protocol
- CLI tool (memrec) for memory management
- 5 memory types: decision, knowledge, context, preference, conversation
- Tag-based categorization with importance weighting
- Automatic long content splitting (>7.5KB)
- Real semantic embeddings with FastEmbed (all-MiniLM-L6-v2, 384 dimensions)
- Local ONNX model loading (~90MB)
- Configurable model path via MEMREC_MODEL_DIR env var
- RocksDB-based vector persistence
- Auto-rebuild missing embeddings on startup
- 30s sync interval + graceful shutdown save
- Systemd user service with install/uninstall scripts
- Skill for AI CLI tools (opencode, claude code)
- Chinese and English README
- 52 unit tests
