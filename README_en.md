[中文](README.md) | English

# MemRec — AI Memory Persistence System

> Local-first AI memory with project isolation — for terminal, for private use

Local memory persistence system for AI CLI tools, providing cross-session memory recovery, knowledge accumulation, and conversation archival.

## Features

- **Project Isolation** — Auto-detect git root, .mr_pid persistence, independent memory per project
- **Hybrid Search** — KNN vector search + BM25 full-text, MMR reranking, Chinese text support
- **Scene Aggregation** — L2 theme-based aggregation over memory cards, heat-ranked (memory pyramid L0-L3)
- **Dream Consolidation** — idle-triggered two-stage consolidation (Stage F fusion + Stage N networking) with optional LLM summaries; every run (incl. gate rejection, dry-run, failure) is audited as a `DreamRun`
- **Retrieval Graph Diffusion** — 1-hop graph diffusion augments hybrid recall; graph hits carry an `EdgeHint`; `retrieval_graph_quota` reserves `top_k` slots for graph hits
- **Offline Retrieval Evaluation** — `memrec eval` reports hit@k/recall@k/MRR/nDCG over a JSONL dataset
- **LLM Integration** — optional OpenAI-compatible LLM for Dream summaries (and optional LLM semantic edges)
- **Write Dedup** — ADD/UPDATE/NOOP decision on every write, with an in-place update window
- **Plugin Ecosystem** — pi extension & DeepSeek Harness native plugin
- **Semantic Search** — Local ONNX model — MiniLM-L6-v2 (384d) or BGE-M3 (1024d, multilingual), zero API cost
- **Smart Scoring** — Time decay, evergreen exemption, source weighting
- **Cross-Project Search** — `--all` flag to discover related knowledge across projects
- **AI-first Design** — JSON output by default, concise commands, Skill integration
- **High Performance** — Rust, <1ms latency, ~118MB (MiniLM) / ~1.5GB (BGE-M3) memory (including model)
- **Auto-splitting** — Long content (>7.5KB) automatically split into chunks
- **Systemd Integration** — Manage daemon with `systemctl --user`
- **Pagination** — `memrec list --skip <n>` for large result sets
- **Soft Delete Recovery** — `memrec list --deleted` + `memrec restore <id>`
- **Tag/Type Filtering** — Filter memories by tags and types on list

## Quick Start

### Install

```bash
# One command to install everything
cargo install --locked mr-install
mr-install
```

`mr-install` automatically:
1. Installs memrec/memrecd via `cargo install`
2. Creates `~/.memrec/` directory structure
3. Downloads ONNX embedding model (~90MB)
4. Registers and starts the daemon service
5. Verifies the installation

| Platform | Binary Path | Data Path |
|----------|------------|-----------|
| Linux | `~/.local/bin/` | `~/.memrec/` |
| macOS | `~/bin/` | `~/.memrec/` |

### Choose Embedding Model

| Model | Dimension | Best For | Disk Space | Memory |
|-------|-----------|----------|------------|--------|
| `minilm-l6-v2` (default) | 384 | English-only | ~90MB | ~118MB |
| `bge-m3` | 1024 | Chinese/multilingual | ~2.3GB | ~1.5GB |

```bash
# Default: MiniLM-L6-v2 (English)
mr-install

# BGE-M3 (Chinese/multilingual, recommended for Chinese users)
mr-install --model bge-m3
```

Mirror options for model download:

```bash
mr-install --use-hf-mirror           # Use hf-mirror.com (China)
mr-install --mirror-base-url <URL>   # Custom mirror
```

### Usage

```bash
# Add memories
memrec add "Choose JWT auth" --mtype decision --tag critical
memrec add "RAII: resource acquisition is initialization" --mtype knowledge --tag best-practice --tag rust
memrec add "User prefers verbose output" --mtype preference --tag output --global

# Hybrid search (KNN + BM25)
memrec search "auth"                        # default scope: project if .mr_pid exists, else global
memrec search "performance" -p              # Current project only
memrec search "preferences" -g              # Global memories only
memrec search "xlsb" -a                     # Across all projects
memrec search "中文搜索" --human            # Chinese text supported

# Other commands
memrec list --limit 20
memrec list --limit 10 --skip 20        # 分页：跳过前 20 条
memrec list --tag rust --type decision   # 按标签/类型过滤
memrec list --deleted                    # 从 RocksDB 检索软删除的原始记录
memrec get <id>
memrec restore <id>                      # 恢复软删除记忆（重新入检索索引）
memrec stats
memrec version

# L2 场景聚合
memrec scene create "Rust coding standards" --tag rust
memrec scene list --sort-by-heat
memrec scene add-memory <scene-id> <memory-id>

# Dream consolidation (idle-triggered, or manual)
memrec dream --force
memrec dream --dry-run                    # dry-run: write nothing, delete nothing, build no edges
memrec dream-report --last 5              # inspect recent DreamRun audit records
memrec eval dataset.jsonl --k 10          # offline retrieval evaluation (JSONL)
```

## Project Isolation

MemRec automatically creates independent memory spaces for different projects:

```
project-a/           project-b/
├── .mr_pid          ├── .mr_pid        ← Auto-created, different IDs
├── .gitignore       ├── .gitignore     ← Add .mr_pid to .gitignore
└── src/             └── src/
```

- **Git repos**: Auto-detect git root
- **Non-git dirs**: Use current working directory
- **Global memories**: `--global` flag, accessible from all projects
- **Cross-project search**: `--all` searches across all projects

## Memory Types

| Type | Flag | Purpose |
|------|------|---------|
| Decision | `decision` | Key technical/business decisions |
| Knowledge | `knowledge` | Knowledge (subdivide via tags: `fact`/`best-practice`/`algorithm`/`tool`) |
| Context | `context` | Project config, environment info |
| Preference | `preference` | User preferences (recommend `--global`) |
| Conversation | `conversation` | Conversation records (default) |

## Scene Aggregation (L2)

Memories are organized as a pyramid: L0 raw conversations → L1 memory cards → L2 scene archives → L3 persona (not yet implemented). The L2 scene layer aggregates L1 cards by theme, ranked by access heat:

```bash
memrec scene create "Rust coding standards" --tag rust   # create a scene
memrec scene list --sort-by-heat                         # heat-ranked list
memrec scene add-memory <scene-id> <memory-id>           # attach a memory card
memrec scene remove-memory <scene-id> <memory-id>
memrec scene get <scene-id>                              # detail with evidence chain
memrec scene update-heat <scene-id> 0.8
```

Each scene carries `theme`, `memory_ids`, `heat` and `tags`; heat rises with memory access, giving hot topics priority visibility. Design: [L2-scene-aggregation-design.md](docs/design/L2-scene-aggregation-design.md)

## LLM Integration (OpenAI-compatible)

Optional OpenAI-compatible LLM integration powers **Dream consolidation summaries**.
When disabled (`[llm].enabled = false`), Dream is treated as disabled.

If the daemon runs as a **systemd user service** (which does not inherit shell env), write the API key to `~/.memrec/memrecd.env` (mode 600; the service unit loads it via `EnvironmentFile`, and the `mr-install` service template already includes this line):

```bash
echo "MEMREC_LLM_KEY=sk-xxx" > ~/.memrec/memrecd.env && chmod 600 ~/.memrec/memrecd.env
systemctl --user restart memrecd
```

When running `memrecd` manually in a terminal, just use the shell env var (`export MEMREC_LLM_KEY=sk-xxx`).
Do not put the key into `~/.config/environment.d/` — that directory is system-wide shared config, and changes only take effect after restarting the user manager.

```toml
# ~/.memrec/config.toml
[llm]
enabled = false            # Enable to unlock Dream summaries
provider = "deepseek"       # deepseek / openai / custom (any OpenAI-compatible)
url = "https://api.deepseek.com/v1"
api_key = ""                # Your API key, supports env var reference (e.g. "$MEMREC_LLM_KEY")
model = "deepseek-chat"     # e.g. deepseek-reasoner, gpt-4o-mini
think_level = "medium"      # off / minimal / low / medium / high / xhigh
timeout_secs = 120
max_tokens = 4096
temperature = 0.3
```

`think_level` maps per provider:

| think_level | DeepSeek (`thinking` param) | OpenAI (`reasoning_effort`) |
|-------------|-----------------------------|-----------------------------|
| `off`       | not sent                    | not sent                    |
| `minimal`/`low`/`medium`/`high` | `{type: enabled, effort: high}` | `minimal`/`low`/`medium`/`high` |
| `xhigh`     | `{type: enabled, effort: max}` | `high`                       |

## Write Deduplication (ADD/UPDATE/NOOP)

On every `add`, MemRec searches similar memories (cosine similarity) and decides:

- `score >= duplicate_threshold` → **skip** (NOOP), memory not written
- `update_threshold <= score < duplicate_threshold` → **update** existing memory (merge tags, new content)
- otherwise → **add** as new memory

`update_window_mins` bounds when an update applies in place: within the window a match updates in place; outside the window MemRec adds a new record and marks the old one `superseded` (merging and soft-deletion are done only by Dream).

```toml
[dedup]
enabled = true
top_k = 5
duplicate_threshold = 0.92   # above this = duplicate, skip
update_threshold = 0.85      # above this = update existing
update_window_mins = 60      # in-place update window (minutes); 0 = unlimited
```

Disable per-call with `--no-dedup` (CLI) or `"dedup": false` (protocol).
The decision layer is pluggable: rule-based today, LLM-based (true DELETE/conflict resolution) is reserved for evolution.

## Dream Consolidation

Dream is a background job that consolidates memories like human sleep does — triggered automatically when the system is idle, or manually via `memrec dream --force`.

Consolidation uses a **two-stage pipeline**:

1. **Stage F (fusion)** — zero-LLM semantic clustering; per-cluster structured extraction of `facts`/`summary`/`contradictions` with provenance (`metadata.source_ids`, used for idempotency and cluster-scoped ratings)
2. **Stage N (networking)** — build `Similar`/`SameProject`/`References`/`Contradicts` edges for retrieval graph diffusion; with `networking_semantic_enabled`, LLM semantic edges (`CausedBy`/`Evolves`/`References`, off by default) are appended

It then runs phases 1-4: `CrossProjectExtract` (cross-project themes), `PersonalSummary` (personal profile), `Cleanup` (stale/low-importance cleanup), `VectorRegen` (missing-embedding regeneration, off by default).

Every run (including gate rejection, dry-run and failure) is persisted as a `DreamRun` audit record, inspectable via `memrec dream-report`; `memrec dream --dry-run` previews without writing, deleting or building edges.

Gate conditions: at least `min_memories` (20) consolidatable memories, oldest memory age ≥ `max_age_hours` (168h), `min_hours_between` (24h) since the last run, and at least `min_new_memories` (1) **user-added** memories since the last run (counted by `created_at > last_run` with non-System source). With `[llm]` configured, consolidation produces an LLM summary memory tagged `dream-integrated`.

**Preserve strategy** (prevent valuable memories from being compressed away):

- `integration_preserve_importance` (0.85) — memories at/above this importance are excluded
- `integration_preserve_tags` (`critical`) — memories matching a tag are excluded
- `integration_preserve_types` (`preference` + `context`) — user preferences and config/environment info stay as-is (context details are irrecoverable after compression)
- `integration_min_content_len` (10 chars) — filters out short test/placeholder garbage
- `integration_max_memories` (100) — input cap per run, selected by ascending importance, prevents LLM token overflow

**Robustness**: `dream.state` is updated only on success (retries are immediate after LLM outages); the integration memory records `metadata.source_ids` for idempotency (crash leftovers are auto-cleaned); phases 1-4 run only after a successful LLM integration; summaries are produced via a strongly-constrained template (`## Topic`/`## Key Points`/`## Conclusion`, no filler).

```toml
[dream]
enabled = false            # enable to unlock background consolidation
requires_llm = true        # gate fails when [llm] not configured
min_memories = 20          # minimum consolidatable memories
max_age_hours = 168        # max age of memories to consolidate (hours)
min_hours_between = 24.0   # min interval between runs (hours)
min_new_memories = 1       # user-added memory gate since last run
integration_max_memories = 100        # input cap per run (ascending importance)
integration_preserve_importance = 0.85  # preserve high-importance memories
integration_preserve_tags = ["critical"]  # preserve matching tags
integration_preserve_types = ["preference", "context"]  # preserved memory types (as-is)
integration_min_content_len = 10      # filter short content (chars)
cleanup_inactive_days = 90  # Phase 3: inactive days threshold
cleanup_importance_threshold = 0.1    # Phase 3: cleanup importance floor
phase_vector_regen = false # Phase 4, resource-heavy, off by default

# Retrieval graph diffusion (1 hop; graph hits carry an EdgeHint)
retrieval_graph_enabled = true   # route retrieval through graph diffusion
retrieval_graph_depth = 1        # diffusion hops
retrieval_graph_decay = 0.6      # diffusion decay
retrieval_graph_weight = 0.3     # weight of graph hits merged into candidates
retrieval_graph_quota = 0.25     # reserved top_k share for graph hits (0 = score-only truncation)

# Networking (Stage N)
networking_enabled = true               # build edges
networking_similar_topk = 5             # Similar-edge nearest neighbours
networking_similar_threshold = 0.82     # Similar-edge threshold
networking_semantic_enabled = false     # LLM semantic edges (off by default)
networking_semantic_max_candidates = 20 # semantic-edge candidate pool capped by vector similarity
networking_semantic_dup_threshold = 0.97 # semantic-edge near-duplicate exclusion threshold
networking_propagation_enabled = true   # edge propagation

# Upstream dedup (Stage F produced facts)
fusion_dedup_enabled = true     # dedup near-duplicate produced facts
fusion_dedup_threshold = 0.97   # near-duplicate threshold (cosine)

[idle]
enabled = true             # idle monitor
check_interval_secs = 300  # check every 5 minutes
load_threshold = 0.5       # idle when load average below this
auto_dream_enabled = true  # auto-trigger Dream when idle

[queue]
normal_queue_capacity = 1000
dream_queue_capacity = 10

[log]
enabled = true
level = "info"             # trace/debug/info/warn/error
rotation_days = 7          # keep 7 days of logs (rotated at local midnight)
memrecd_log = "~/.memrec/logs/memrecd.log"  # full log path (local-timezone timestamps)
```

## Plugins

| Plugin | Platform | Capabilities |
|--------|----------|--------------|
| `pi-extensions/memrec.ts` | pi coding agent | 6 LLM tools (`memrec_search`/`add`/`get`/`list`/`delete`/`stats`) + auto context injection on agent start; commands `/memrec`, `/memrec-auto` |
| `dsh-plugin/` | DeepSeek Harness | 7 native tools (`mr_add`/`mr_search`/`mr_get`/`mr_list`/`mr_delete`/`mr_stats`/`mr_dream`) + context injection + auto session recording (LLM-extracted) |

- **pi**: copy `pi-extensions/memrec.ts` to `~/.pi/agent/extensions/`, then `/reload`
- **DSH**: see [dsh-plugin/README.md](dsh-plugin/README.md)

## Data Location

```
~/.memrec/
├── config.toml           # Configuration
├── memrecd.sock          # Unix Socket
├── data/                 # RocksDB memory metadata
├── vectors/              # RocksDB vector storage
├── fts/                  # Tantivy full-text search index
├── logs/                 # Log files
│   ├── memrecd.log       # Daemon log (no ANSI colors)
│   └── dream.log         # Dream consolidation job log
└── models/               # ONNX Embedding model
    ├── Qdrant--all-MiniLM-L6-v2-onnx/   # MiniLM-L6-v2 (default)
    │   ├── model.onnx
    │   ├── tokenizer.json
    │   └── ...
    └── BAAI--bge-m3/                     # BGE-M3 (multilingual)
        ├── model.onnx
        ├── model.onnx_data
        ├── tokenizer.json
        └── ...
```

## Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `MEMREC_MODEL_DIR` | Custom model path | `~/.memrec/models/<model-dir>/` (model-specific) |
| `MEMREC_MIN_SCORE` | Min similarity score | `0.75 (MiniLM) / 0.5 (BGE-M3)` |
| `MEMREC_LLM_KEY` | LLM API key (`[llm] api_key` may reference `"$MEMREC_LLM_KEY"`; systemd service loads it from `~/.memrec/memrecd.env`) | empty |
| `RUST_LOG` | Log level | `info` |

## Documentation

- [Manual](MANUAL_en.md)
- [Manual (Chinese)](MANUAL.md)
- [Skill Documentation](skills/memrec/SKILL.md)
- [L2 Scene Aggregation Design](docs/design/L2-scene-aggregation-design.md)
- [DSH Plugin Design](docs/design/dsh-plugin-design.md)
- [Idle-triggered Dream Design](docs/superpowers/specs/2026-07-30-idle-dream-design.md)

## Project Structure

```
memrec/
├── common/       # Shared types and protocol
├── memrecd/      # Daemon service
├── memrec/       # CLI tool
├── mr-install/   # Installer
├── mr-ability/   # Core library (search/embedding/dream/llm/dedup/queue/idle)
├── mr-common/    # Shared types
├── mr-protocol/  # JSON-RPC protocol
├── dsh-plugin/   # DeepSeek Harness plugin
├── pi-extensions/# pi extension
└── docs/         # Documentation
```

## License

Apache-2.0

## Changelog

See [CHANGELOG.md](CHANGELOG.md) (中文版 [CHANGELOG_en.md](CHANGELOG_en.md))

