# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> [中文](CHANGELOG.md) | English

## [Unreleased]

### Added

- `memrec eval` gains `--alpha` and `--no-mmr` to isolate pure-vector/pure-full-text retrieval and MMR-off ordering
- Retrieval quality gate `scripts/retrieval_gate.sh` with golden set `eval/retrieval_golden.jsonl` (self-retrieval invariant + paraphrase hit@5/MRR), wired into the multi-step test and deploy flow
- **One-off historical project-attribution migration tool** (not a permanent API): `mr-ability/examples/migrate_project_attribution.rs` plus `scripts/project_migration/` (`build_mapping.py` scans disk `.mr_pid` files, with a directory-name alias table `aliases.json`); reassigns historically mis-attributed memories from the single bucket back by tag/alias, with two-phase `dry-run`/`apply`, `--assign` explicit assignment and `--restore-deleted`; migration goes through `IndexingStorage::save` to refresh the `by_project` secondary index, vector payload and FTS payload consistently and flush to disk, and a report records `(id, source, target)` for audit and reverse migration

### Changed

- **Large-file split (pure structural move, no behavior change)**: `memrecd/src/server/handler.rs` (2679 lines) split into per-RPC-domain submodules and `mr-ability/src/dream/processor.rs` (1635 lines) split into submodules with `process_inner` decomposed; `memrec/src/mcp/mod.rs`, `mr-common/src/types/config.rs`, `mr-ability/src/storage/memory_store.rs`, `mr-protocol/src/request.rs` and `memrec/src/commands/memory.rs` became directory modules; every production file is ≤ 500 lines
- **Retrieval-path performance**: new `MemoryStorage::get_many` (backed by `RocksDBStore::multi_get_cf`) batches search and graph-diffusion fetches, removing N+1; vector rebuild `rebuild_missing_embeddings` now uses `embed_batch`; the vector search narrows its lock scope (sorting moved outside the lock, two-phase locking)
- **Code-quality cleanup**: `cargo clippy --release --all-targets` is at zero warnings; production `expect`s removed (`mr-ability/src/llm/client.rs` returns `Result`, `memrecd/src/config.rs` logs and falls back); inline FQNs replaced with short `use` names; `partial_cmp().unwrap()` NaN panics eliminated; `parse_relations` parse failures warn (with char-safe truncation); `stats` drops the `storage_usage`/`avg_importance` fields that were long hardcoded to 0.0 (no reliable data source)
- **Log noise reduction**: expected Dream gate skips and sync noise downgraded to debug

### Fixed

- **Inverted vector normalization**: `HybridStore::merge_and_normalize` applied `1.0 - normalized` to vector similarity, ranking the most relevant memory last. With same-direction normalization, pure-vector self-retrieval recall@5 rose from 0.36 to 1.0, and golden-set MRR from 0.251 to 0.711
- **Chinese full-text tokenization**: The Tantivy full-text field now uses jieba-rs word-level tokenization (was n-gram), so paraphrase-style Chinese queries can match the source; the FTS index is automatically rebuilt when the tokenizer semantics change (memory store remains the source of truth)
- **Special characters aborting search**: Queries containing `+ - : ( ) " [ ]` made `QueryParser` fail and abort the whole search; it now falls back to an OR query built from the tokenizer tokens
- **MCP writes not attributed to the caller's project**: `mr_add` always passed `working_dir: None` for non-global writes, so the daemon fell back to its own cwd to resolve the project, funneling every MCP write into a single project bucket (project isolation failure; the read paths and the CLI write path were correct). It now aligns with `mr_search`/CLI, passing the caller's working directory for non-global memories, with an attribution-consistency regression test added
- **Fused score double-filtered by the same threshold**: `min_score` was defined as the raw vector-cosine floor, yet the same absolute threshold was applied again to the min-max normalized and `alpha`-weighted fused score, pinning a pure-vector top hit at `alpha` (default 0.5) — exactly the default `min_score` — and truncating results to the top hit only (scores "stuck at 0.5", no discrimination). `min_score` now constrains raw vector cosine only; the fused layer no longer truncates absolutely, so the hit set is bounded by the vector absolute threshold and `top_k`
- **Empty content preview for pure full-text hits**: the Tantivy `content` field was indexed but not `STORED`, so full-text hits without a vector payload always had an empty `content_preview`, weakening an agent's ability to judge relevance; it is now `STORED` and backfilled on hit (truncated to 200 chars, matching the vector-side payload), with the index version marker bumped to `cn_jieba_v2` to trigger a rebuild of old indexes
- **Truncated large responses**: the CLI and MCP IPC response cap was raised from 1MB to 64MB (an upper bound is still kept to defend against abnormally large responses), so legitimate bulk list queries are no longer rejected (measured `list -a -l 400` ≈ 3.7MB)

## [0.9.0] - 2026-10-18

### Added

- **Two-stage Dream refactor**: Dream is split into Stage F (fusion: zero-LLM semantic clustering + per-cluster structured extraction of `facts`/`summary`/`contradictions` + provenance) and Stage N (networking: build `Similar`/`SameProject`/`References`/`Contradicts` edges)
- **DreamRun audit**: every Dream run (including gate rejection, dry-run and failure) is persisted as a `DreamRun` (new RocksDB `dream_runs` column family), inspectable via `memrec dream-report`
- **Dream dry-run**: `memrec dream --dry-run` computes only — writes nothing, deletes nothing, builds no edges
- **Graph diffusion in retrieval**: after hybrid search, supplement recall with 1-hop graph diffusion (`Similar`/`References`/`CausedBy`/`Evolves`); graph hits carry an `EdgeHint`; soft-deleted memories are used only for connectivity, never returned
- **Graph-hit quota**: `retrieval_graph_quota` (default 0.25) reserves slots for graph hits within `top_k`, so they are not truncated away because their score is always below the seed score (`quota=0` falls back to score-only merge/truncation)
- **LLM semantic edges** (off by default): with `networking_semantic_enabled`, an LLM decides `CausedBy`/`Evolves`/`References`; the candidate pool is limited to the vector top-k (`networking_semantic_max_candidates`, default 20) and excludes near-duplicates and self (`networking_semantic_dup_threshold`, default 0.97)
- **Offline retrieval evaluation**: `memrec eval <dataset>` drives read-only retrieval over a JSONL dataset and reports `hit@k`/`recall@k`/`MRR`/`nDCG`
- **Record update window**: `[dedup] update_window_mins` (default 60) — similar writes within the window update in place; outside the window a new record is added and the old one is marked `superseded`
- **Upstream dedup**: embedding-based near-duplicate dedup of Dream-produced facts (`fusion_dedup_enabled`/`fusion_dedup_threshold`, default 0.97)
- **Startup full-text index self-heal**: on startup, reconcile the full-text index against the active memory count; on mismatch, rebuild from the memory store as the source of truth (clearing stale deleted, duplicate and dangling docs)
- **Secondary-index pushdown queries**: two new RocksDB secondary index column families, `by_created` (unix seconds big-endian + id) and `by_project` (project bucket + id); Dream candidate time ranges and global/project memory listings are pushed down to index-prefix scans (second-level key truncation is compensated by exact re-filtering on the caller side), replacing full scans with in-memory filtering; missing index entries are self-healed at startup

### Changed

- **Importance semantics**: `importance` is a global prior only; Dream ratings are cluster-scoped (topic-scoped), attached to cluster outputs and never written back to the open field
- **Dream configuration expanded**: new `[dream]` keys `networking_*`, `retrieval_graph_*` (including `retrieval_graph_quota`) and `fusion_dedup_*`
- **Unified global semantics**: `project_id = None` is now the single global-memory criterion, and `MemoryScope::Workspace` is deprecated (the enum remains only for persisted-data compatibility); a new explicit `SearchFilter.global_only` is honored consistently by both vector and FTS sides; all write paths (Add, Dream outputs, personal summarizer, CLI `Scope::Global`) normalized to None; legacy `Some(nil)` and `Workspace` records (including soft-deleted) are migrated to None/Global at startup, idempotently
- **Typed daemon calls from MCP**: `call_daemon` now takes a `JsonRpcRequest` (enum method + typed params union) and serializes it directly, removing the "method string + type field" double bookkeeping; a missing mapping for a new request becomes a compile-time error
- **handle_add split**: split into five single-purpose steps — memory construction / dedup decision / in-window merge / supersede marking / persist+index; the correction fallback semantics (out of window → mark `superseded` → normal add) are unchanged

### Fixed

- **MMR diversity failure**: re-ranking similarity used the 200-char `content_preview`, so memories with identical openings but opposite conclusions were judged near-identical; a new `TextProvider` trait fetches full texts in batch before re-ranking (falls back to the old behavior when no provider is set)
- **WriteBatch index erasure in save()**: puts came before deletes in the batch, so same-key moves (nil bucket == global bucket) erased freshly written index entries; stale-index cleanup now precedes writes
- **Lost error chains in Dream**: `DreamError::Storage`/`Llm` variants carried Strings, losing root causes; now `#[from] anyhow::Error` and `#[from] LlmError`, with `#[error]` interpolating `{:#}` to render the full chain; `extract_cluster` propagates `LlmError` directly
- **FTS global search let all documents through**: the full-text global_only query never filtered because the `include_global` branch was always true; fixed alongside the explicit `SearchFilter.global_only`
- **Removed dead interfaces**: the never-implemented and never-referenced `ProjectStorage`/`ConfigStorage` traits are deleted

- **Dream write path bypassed the hybrid index**: produced memories were unsearchable and soft-deleted sources appeared as "ghost" results; a new `IndexingStorage` decorator mirrors all Dream writes into the hybrid index, and the startup vector rebuild's `list(1000)` truncation was fixed to full scan
- **Non-deterministic hybrid ranking**: added a `memory_id` tie-break for equal scores so evaluation results are reproducible
- **LLM semantic-edge self-loops**: the new memory itself is now skipped in the candidate pool
- **Global nil-project `SameProject` fan-out**: `SameProject` edges skip global (nil) projects, avoiding O(n²) fan-out between produced memories
- **Gate ordering**: the `min_memories` check now precedes the `integration_max_memories` truncation, removing the spurious "not enough memories" report when `max < min`

### Docs

- New absorption-analysis designs: Hy Memory, MemHub/One Memory, MemOS 2.0/Mem0 (with explicit rejections)
- New Dream refactor plan and strategy-A design: `docs/design/dream-refactor-design.md`, `docs/design/dream-refactor-strategy-a.md`
- New graph-diffusion quantification and semantic-edge quality report: `docs/design/dream-graph-quantification.md`

## [0.8.0] - 2026-08-30

### Added

- **Three-state scope options**: `list` / `search` replace `--project-only` / `--global-only` with mutually exclusive `-p/--project` (current project only), `-g/--global` (global memories only), `-a/--all` (across all projects); when none is given, the scope is inferred from the presence of the `.mr_pid` marker in the project root (present→`-p`, absent→`-g`); `list -p` now resolves the project UUID from `.mr_pid` to scope precisely to the current project (previously listed memories of all projects); MCP tools `mr_search`/`mr_list` aligned (a `scope` enum replaces the old `project_only`/`global_only`/`cross_project` booleans, inferred from `.mr_pid` when omitted; `mr_search` project scope now passes `working_dir`, fixing wrong-project resolution caused by the daemon using its own cwd)
- **[llm] api_key supports env var reference**: `api_key = "$MEMREC_LLM_KEY"` (or `${MEMREC_LLM_KEY}`) is expanded to the environment variable value when the daemon loads config.toml, avoiding plaintext keys on disk; keeps the original value if the variable is unset
- **Pagination**: `ListParams.skip` / `memrec list --skip <n>` — page through memories beyond the latest N; `total` now returns the full count of matching records (accurate next-page detection)
- **Raw record retrieval & restore**: inspect and recover soft-deleted memories from RocksDB
  - `memrec list --deleted`: list soft-deleted raw records (RocksDB `cf_deleted` column family)
  - `memrec list --tag <t> --type <t>`: tag/type filtering (protocol `ListParams.tags`/`memory_type` now actually enforced)
  - `memrec restore <id>`: restore a soft-deleted memory and re-add it to the hybrid search index

### Fixed

- **DeepSeek thinking-mode params corrected** (per official thinking_mode docs): thinking is **enabled by default**, so `off` must explicitly send `{"thinking":{"type":"disabled"}}` (previously omitting it still left deep thinking on, polluting Dream summaries with reasoning output); the effort param moved to top-level `reasoning_effort` (was wrongly nested in `thinking.effort`), mapping `low→low`, `medium→high` (official), `high→high`, `xhigh→max`; **Dream summary calls now force thinking off** (`chat_with_think(Off)`) — template-driven generation needs no reasoning
- **Documentation naming unified (Chinese as default)**: `README_cn.md`→`README.md`, `README.md`→`README_en.md`; `CHANGELOG_cn.md`→`CHANGELOG.md`, `CHANGELOG.md`→`CHANGELOG_en.md`; `MANUAL_cn.md`→`MANUAL.md`, `MANUAL.md`→`MANUAL_en.md`; language-switch links added at the top of each doc, all references updated
- **systemd service template supports env file**: the `memrecd.service` registered by `mr-install` now includes `EnvironmentFile=-~/.memrec/memrecd.env` (optional user env file, mode 600), fixing LLM 401s caused by systemd services not inheriting shell env vars (e.g. `MEMREC_LLM_KEY`); README env-var guidance updated accordingly (recommend `memrecd.env`, stop recommending the shared `environment.d`)
- **Dream integration preserve strategy extended**: new `integration_preserve_types` (default preserves `preference` + `context`) — context config/environment info (file paths, verification data, progress) loses irrecoverable details when compressed, so it is excluded from integration by default; new `integration_min_content_len` (default 10 chars) filters out short test/placeholder garbage
- **Logs now use local timezone (+8)**: timestamps switch from UTC to local time (with offset); daily rotation boundary moves from UTC midnight to local midnight (previously the Aug 30 early-morning entries still went into the Aug 29 file); replaced `tracing-appender` with a self-implemented `LocalDailyWriter` (tracing-appender rotates on UTC and cannot be configured), file names remain `memrecd.YYYY-MM-DD.log`, and stale-file cleanup now sorts by file name date (mtime is unreliable for batch writes)
- **Dream integration robustness & quality** (P1-P7):
  - **P1 input cap**: new `integration_max_memories` (default 100); single LLM integration truncates by importance, preventing token window overflow from concatenating thousands of memories
  - **P2 no state write on failure**: `dream.state` is written only on success, so retries are immediate after LLM outage instead of wasting the 24h window
  - **P3 prioritized selection**: candidates are now full-scanned and sorted by importance ascending (low-value first) instead of approximate-random UUID order; also bypasses the internal 10k scan cap of `list_older_than`
  - **P4 preserve high value**: new `integration_preserve_importance` (default 0.85) and `integration_preserve_tags` (default `critical`); memories at/above the threshold, matching a preserve tag, or of Preference type are excluded from integration and deletion
  - **P5 idempotency**: integration memory records source memory ids in `metadata.source_ids`; next run excludes already-integrated sources and cleans up crash leftovers (written but not deleted)
  - **P6 phase ordering**: Phase 1-4 run only after a successful LLM integration; no phase side effects on failure
  - **P7 new-memory gate**: gate now counts user-added memories since last run (`created_at > last_run_at`, non-System source) instead of net total delta (which misfired on delete/add offsets and counted Dream's own products); cleanup params (`cleanup_inactive_days`/`cleanup_importance_threshold`) configurable
- **Dream summary output quality**: LLM System Prompt switched to a strongly-constrained template (`## Topic` / `## Key Points` / `## Conclusion` fixed structure), hard-banning openings, closing pleasantries and vague filler; content must come from input memories, language matches the input, max 500 chars; new `clean_summary` post-processing strips Markdown code fences and common small-talk prefixes (e.g. "以下是…" "总结如下" "好的，"), compresses redundant blank lines
- **Dream garbage memory elimination** (v0.7.0 leftover: duplicate `personal-summary`/`cross-project` summaries polluted the memory base, 94% of 100 memories were machine-generated):
  - Phase 1/2 summaries switched to upsert: keep only the latest one per tag, update only on content change, roll back extra stale entries
  - DreamGate time gate now reads persisted `dream.state`, decoupled from the concurrency lock (previously the timestamp was lost on lock release, so the 24h gate never applied and Dream ran at high frequency)
  - Dream statistical summaries use `source=System` and importance 0.3 (previously defaulted to `User`/0.8, polluting search weights)
  - `process_inner` filters old memories first and skips all phases when below `min_memories` (previously phases 1/2 ran before the LLM-integration check, so garbage was written even when integration was skipped)
  - Cleanup adds summary rollback as a safety net (keep the latest one per tag)
  - Legacy cleanup: removed 1330 duplicate summaries (active 1407 → 77)
- Fixed internal dependency version mismatch after the 0.8.0-dev bump (`mr-common` deps declared 0.7.0-dev → 0.8.0-dev)

---

## [0.7.0] - 2026-08-21

### Added

- **L2 Scene Aggregation**: theme-based aggregation layer over L1 memory cards
  - `memrec scene` subcommands: `create` / `get` / `list` / `delete` / `add-memory` / `remove-memory` / `update-heat`
  - `Scene` entity (`theme`, `memory_ids`, `heat`, `tags`) with heat-based ranking, memory pyramid L0-L3
  - `SceneStore` backed by RocksDB, shared instance with `MemoryStore`
- **Dream Consolidation**: background memory consolidation (sleep-like)
  - 4 phase processors: `CrossProjectExtract` / `PersonalSummary` / `Cleanup` / `VectorRegen` (off by default)
  - `memrec dream --force` manual trigger; `DreamGate` (min 20 memories, max 168h age, 24h interval) + `DreamLock`
  - LLM-powered integration summaries (requires `[llm]` config), tagged `dream-integrated`
  - Auto-triggered by `SystemIdleMonitor` when system is idle (load average < 0.5)
- **LLM Integration (OpenAI-compatible)**: `[llm]` config section
  - `LlmClient` trait with `OpenAiCompatibleClient` (reqwest) and `MockLlmClient`
  - Providers `deepseek` / `openai` / `custom`; `think_level` mapping (DeepSeek `thinking` effort, OpenAI `reasoning_effort`)
- **Write Deduplication**: rule-based dedup decider on every `add`
  - `score >= duplicate_threshold` (0.92) → skip (NOOP), `>= update_threshold` (0.85) → update, else add
  - `--no-dedup` CLI flag and `dedup: false` protocol option; LLM-based decider reserved for evolution
- **SystemIdleMonitor**: load-average idle detection (5min interval, threshold 0.5), auto-triggers Dream
- **QueueDispatcher**: dual-queue scheduler (normal 1000 / dream 10 capacity)
- **Log rotation**: `[log]` config — `memrecd.log` / `dream.log`, 7-day retention, optional compression
- **dsh-plugin**: native MemRec plugin for DeepSeek Harness
  - 7 native tools: `mr_add` / `mr_search` / `mr_get` / `mr_list` / `mr_delete` / `mr_stats` / `mr_dream`
  - Context injection (`agent/pre-step` hook) + auto session recording (LLM-extracted, fire-and-forget)
  - Fixes project isolation lost in MCP mode by passing `working_dir`
- **pi extension**: `pi-extensions/memrec.ts`
  - 6 LLM tools: `memrec_search` / `memrec_add` / `memrec_get` / `memrec_list` / `memrec_delete` / `memrec_stats`
  - Auto memory injection on agent start; `/memrec` and `/memrec-auto` commands
- **Thread naming** for tokio workers
- New config types: `LlmConfig`, `DedupConfig`, `DreamConfig`, `IdleMonitorConfig`, `QueueConfig`, `LogConfig`

### Changed

- RocksDB instance shared between `MemoryStore` and `SceneStore` (single open)
- `mr-install` default model switched to BGE-M3 to match memrecd
- `add` parameters encapsulated into `AddOptions` struct
- Tests use `MockEmbedder`; model-dependent tests marked ignored
- Log file path resolution and color rendering fixes

### Fixed

- Deleted memories no longer returned by hybrid search (search index removed on delete)
- `min_score` now applied to fused hybrid scores
- Empty previews filtered in plugin context injection
- Clippy warnings

### Docs

- `docs/design/L2-scene-aggregation-design.md`
- `docs/design/dsh-plugin-design.md`
- `docs/superpowers/specs/2026-07-30-idle-dream-design.md`
- `dsh-plugin/README.md`

---

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
- Comprehensive user manual: `MANUAL_en.md` (English) and `MANUAL.md` (Chinese)

### Changed

- Default min_score: 0.75 → 0.5 (BGE-M3 produces lower cosine similarity; MiniLM still uses 0.75 via `ModelType::default_min_score()`)
- Config format: flat `name = "Qdrant/all-MiniLM-L6-v2-onnx"` → structured `[model]` section with `model_type` and `[[model.files]]`
- `mr-install` now generates config based on selected model type
- Vector dimension: 384 (MiniLM) or 1024 (BGE-M3), determined by model selection
- Memory usage: ~118MB (MiniLM) / ~1.5GB (BGE-M3) at runtime
- Model download size: ~90MB (MiniLM) / ~2.3GB (BGE-M3)
- Documentation consolidated: `docs/user-guide.md` and `docs/installation.md` merged into `MANUAL_en.md` / `MANUAL.md`
- README documentation links updated to point to MANUAL files

### Fixed

- Search returning 0 results with BGE-M3: adjusted default min_score from 0.75 to 0.5
- External ONNX data loading: `with_external_initializer()` for BGE-M3's `model.onnx_data` and `Constant_7_attr__value`
- BGE-M3 pooling: CLS pooling instead of Mean for correct embeddings

### Removed

- Old flat config format (`name = "Qdrant/all-MiniLM-L6-v2-onnx"` in config.toml)
- Hardcoded model path assumptions
- `docs/user-guide.md` (superseded by `MANUAL.md`)
- `docs/installation.md` (superseded by `MANUAL_en.md` / `MANUAL.md`)

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
