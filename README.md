[中文版](README_cn.md) | English

# MemRec — AI Memory Persistence System

> Local-first AI memory with project isolation — for terminal, for private use

Local memory persistence system for AI CLI tools, providing cross-session memory recovery, knowledge accumulation, and conversation archival.

## Features

- **Project Isolation** — Auto-detect git root, .mr_pid persistence, independent memory per project
- **Hybrid Search** — KNN vector search + BM25 full-text, MMR reranking, Chinese text support
- **Scene Aggregation** — L2 theme-based aggregation over memory cards, heat-ranked (memory pyramid L0-L3)
- **Dream Consolidation** — idle-triggered 4-phase memory consolidation with optional LLM summaries
- **LLM Integration** — optional OpenAI-compatible LLM for Dream summaries
- **Write Dedup** — ADD/UPDATE/NOOP decision on every write
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
memrec search "auth"                        # min_score default: 0.5 (BGE-M3) / 0.75 (MiniLM)
memrec search "performance" --project-only  # Current project only
memrec search "preferences" --global-only   # Global memories only
memrec search "xlsb" --all                  # Across all projects
memrec search "中文搜索" --human             # Chinese text supported

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

# Dream 整合（空闲自动触发，或手动）
memrec dream --force
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

```toml
# ~/.memrec/config.toml
[llm]
enabled = false            # Enable to unlock Dream summaries
provider = "deepseek"       # deepseek / openai / custom (any OpenAI-compatible)
url = "https://api.deepseek.com/v1"
api_key = ""                # Your API key
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

```toml
[dedup]
enabled = true
top_k = 5
duplicate_threshold = 0.92   # above this = duplicate, skip
update_threshold = 0.85      # above this = update existing
```

Disable per-call with `--no-dedup` (CLI) or `"dedup": false` (protocol).
The decision layer is pluggable: rule-based today, LLM-based (true DELETE/conflict resolution) is reserved for evolution.

## Dream Consolidation

Dream is a background job that consolidates memories like human sleep does — triggered automatically when the system is idle, or manually via `memrec dream --force`. It runs 4 phases:

1. **CrossProjectExtract** — extract cross-project common themes
2. **PersonalSummary** — aggregate global memories into a personal profile
3. **Cleanup** — remove stale / low-importance memories
4. **VectorRegen** — regenerate missing embeddings (off by default, heavy)

Gate conditions: at least `min_memories` (20) memories, oldest memory age ≥ `max_age_hours` (168h), and `min_hours_between` (24h) since the last run. With `[llm]` configured, consolidation produces an LLM summary memory tagged `dream-integrated`.

```toml
[dream]
enabled = false            # enable to unlock background consolidation
requires_llm = true        # gate fails when [llm] not configured
min_memories = 20
max_age_hours = 168
min_hours_between = 24.0
phase_vector_regen = false # Phase 4, resource-heavy, off by default

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
rotation_days = 7          # keep 7 days of logs
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
| `RUST_LOG` | Log level | `info` |

## Documentation

- [Manual](MANUAL.md)
- [Manual (Chinese)](MANUAL_cn.md)
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

See [CHANGELOG.md](CHANGELOG.md)
