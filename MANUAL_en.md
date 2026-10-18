# MemRec User Manual

> [中文](MANUAL.md) | English

## Overview

MemRec is a local-first AI memory persistence system for AI CLI tools. It provides cross-session memory recovery, knowledge accumulation, and conversation archival with project isolation and semantic search.

**Core principle:** AI-first — default JSON output, concise commands, designed for Agents.

## Installation

### Requirements

| Item | Requirement |
|------|-------------|
| OS | Linux / macOS |
| Rust | 1.75+ (only needed for `mr-install` first-time install) |
| Disk | ~200MB (MiniLM) / ~2.5GB (BGE-M3) including model |
| Memory | ~118MB (MiniLM) / ~1.5GB (BGE-M3) at runtime |

### One-Command Install

```bash
cargo install --locked mr-install
mr-install
```

`mr-install` automatically:

1. Installs memrec/memrecd via `cargo install`
2. Creates `~/.memrec/` directory structure
3. Downloads ONNX embedding model
4. Registers and starts the daemon service
5. Verifies the installation

### Choosing an Embedding Model

| Model | Dimension | Best For | Disk Space | Memory | Default min_score |
|-------|-----------|----------|------------|--------|-------------------|
| `minilm-l6-v2` (default) | 384 | English-only | ~90MB | ~118MB | 0.75 |
| `bge-m3` | 1024 | Chinese/multilingual | ~2.3GB | ~1.5GB | 0.5 |

```bash
# Default: MiniLM-L6-v2 (English)
mr-install

# BGE-M3 (Chinese/multilingual, recommended for Chinese users)
mr-install --model bge-m3
```

**Why different min_score defaults?** BGE-M3 produces inherently lower cosine similarity scores than MiniLM. With MiniLM, exact matches score >0.8; with BGE-M3, exact matches score ~0.74. The 0.5 default for BGE-M3 ensures relevant results are not filtered out.

### Mirror Options (China)

```bash
mr-install --use-hf-mirror           # Use hf-mirror.com
mr-install --mirror-base-url <URL>   # Custom mirror
```

### Skip Steps

```bash
mr-install --skip-install   # Skip cargo install (binaries already installed)
mr-install --skip-model     # Skip model download
mr-install --skip-service   # Skip service registration
mr-install --skip-verify    # Skip verification tests
```

## Service Management

### Linux (systemd)

```bash
systemctl --user status memrecd     # Check status
systemctl --user stop memrecd       # Stop
systemctl --user restart memrecd    # Restart
journalctl --user -u memrecd -f     # View logs
```

Service file: `~/.config/systemd/user/memrecd.service`

### macOS (launchd)

```bash
launchctl list cn.itcraft.memrecd               # Check status
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/cn.itcraft.memrecd.plist   # Stop
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/cn.itcraft.memrecd.plist # Start
```

Config file: `~/Library/LaunchAgents/cn.itcraft.memrecd.plist`

Features: RunAtLoad + KeepAlive — starts on login, auto-restarts on crash.

### Important: Restarting the Daemon

When restarting memrecd, you must remove the stale socket file first:

```bash
rm -f ~/.memrec/memrecd.sock
systemctl --user restart memrecd   # or: memrecd
```

With BGE-M3, model loading + vector rebuild takes ~75-90 seconds before the socket is ready.

## Commands

### Command Reference

| Command | Purpose |
|---------|---------|
| `memrec add` | Add a memory |
| `memrec search` | Semantic search |
| `memrec get` | Get a single memory |
| `memrec list` | List memories |
| `memrec delete` | Delete a memory |
| `memrec restore` | Restore a soft-deleted memory (re-added to the index) |
| `memrec stats` | Statistics |
| `memrec scene` | L2 scene aggregation (create/list/get/add-memory/remove-memory/update-heat/delete) |
| `memrec dream` | Trigger Dream consolidation (`--force`, `--dry-run`) |
| `memrec dream-report` | Inspect recent DreamRun audit records |
| `memrec eval` | Offline retrieval evaluation (JSONL dataset, hit-rate metrics) |
| `memrec version` | Version info |

### Adding Memories

```bash
memrec add "content" --mtype <type> [--tag <tag>] [--global] [--source <source>] [--scope <scope>]
```

#### Memory Types

| Type | Flag | Purpose | Recommended Tags |
|------|------|---------|-----------------|
| Decision | `decision` | Key technical/business decisions | `--tag critical` |
| Knowledge | `knowledge` | Knowledge points, best practices, facts | See subcategories below |
| Context | `context` | Project config, environment info | Project-related tags |
| Preference | `preference` | User preferences | `--global` |
| Conversation | `conversation` | Conversation records (default) | - |

#### Knowledge Subcategories (via tags)

| Tag | Purpose | Example |
|-----|---------|---------|
| `fact` | Physical laws, math formulas, objective facts | `--tag fact --tag physics` |
| `best-practice` | Best practices, design patterns | `--tag best-practice` |
| `algorithm` | Algorithms, formula derivations | `--tag algorithm` |
| `tool` | Tool usage tips | `--tag tool --tag rust` |

#### Examples

```bash
# Decision
memrec add "Choose JWT auth: stateless, easy to scale" --mtype decision --tag auth --tag critical

# Fact
memrec add "Speed of light c=3×10⁸m/s, constant in vacuum" --mtype knowledge --tag fact --tag physics

# Best practice
memrec add "RAII: resource acquisition is initialization, destructor auto-releases" --mtype knowledge --tag best-practice --tag rust

# Project context
memrec add "Tech stack: Rust+Tokio+RocksDB, communication: Unix Socket" --mtype context --tag tech

# User preference (global memory)
memrec add "User prefers verbose output" --mtype preference --tag output --global
```

#### Global vs Project Memories

```bash
# Project memory (default): only searchable from current project
memrec add "Project A database selection" --mtype decision --tag critical

# Global memory: searchable from all projects
memrec add "User prefers dark theme" --mtype preference --tag ui --global
```

#### Source and Scope

| Flag | Values | Description |
|------|--------|-------------|
| `--source` | `user` (default), `system`, `inferred`, `external` | Memory origin — affects search ranking |
| `--scope` | `project` (default), `global` | Memory visibility (legacy `workspace` is deprecated: the enum value exists only for persisted-data compatibility, and startup migration folds it into `global`) |

```bash
# User-sourced memory (highest search weight)
memrec add "My preference: use tabs not spaces" --mtype preference --source user --global

# System-sourced memory
memrec add "Auto-detected: project uses Rust 1.75" --mtype context --source system

# Inferred knowledge
memrec add "Likely prefers functional style based on code patterns" --mtype knowledge --source inferred
```

#### Auto-Splitting Long Content

Content exceeding 7.5KB is automatically split into chunks:

```bash
memrec add "Very long content..." --mtype knowledge
# WARN: Content too long (12.5KB > 7.5KB), auto-splitting into chunks...
# WARN: Split into 2 parts
# Part 1: Added 550e8400-...
# Part 2: Added 6ba7b810-...
# All 2 parts added: [550e8400-..., 6ba7b810-...]
```

Each chunk generates its own embedding and shares a `chunk_group_id`. Use `--merge` to get the full content.

### Hybrid Search

MemRec combines KNN vector search with BM25 full-text search for optimal results.

```bash
memrec search "query" [options]
```

#### Search Pipeline

1. **KNN + BM25**: Parallel search, merge and normalize scores
2. **Time Decay**: Recent memories ranked higher (knowledge/decision exempt)
3. **Source Weighting**: User memories weighted higher than system/inferred
4. **MMR Reranking**: Diverse results, reduce redundancy

#### Chinese Text Support

Chinese text search is supported via n-gram tokenizer (2-4 grams). No additional configuration needed.

```bash
memrec search "中文搜索" --human
```

#### Search Scope

| Option | Scope | Purpose |
|--------|-------|---------|
| (default) | Inferred from `.mr_pid` | Registered project→project only; otherwise→global only |
| `-p, --project` | Current project only | Precise project search |
| `-g, --global` | Global memories only | Find user preferences |
| `-a, --all` | All projects | Cross-project discovery |

> The three scope options are mutually exclusive. When none is given, the CLI checks
> for a `.mr_pid` marker file in the project root: present → `-p` (project only),
> absent → `-g` (global only).

#### Options

| Option | Description | Default |
|--------|-------------|---------|
| `-k, --top-k` | Number of results | 10 |
| `--min-score` | Minimum similarity threshold | 0.5 (BGE-M3) / 0.75 (MiniLM) |
| `-p, --project` | Current project only | Inferred from `.mr_pid` |
| `-g, --global` | Global memories only | Inferred from `.mr_pid` |
| `-a, --all` | All projects | - |
| `--mtype` | Filter by type | - |
| `--human` | Human-readable output | - |
| `--hybrid-alpha` | KNN vs BM25 weight (0=BM25 only, 1=KNN only) | 0.5 |
| `--mmr-enabled` | Enable MMR reranking | true |
| `--mmr-lambda` | MMR diversity (0=max diversity, 1=max relevance) | 0.7 |

#### Examples

```bash
# Basic search
memrec search "auth"

# Project-only search
memrec search "performance" -p

# Global memories
memrec search "preferences" -g

# Cross-project search
memrec search "xlsb" -a

# Adjust result count and threshold
memrec search "architecture" -k 20 --min-score 0.6

# Filter by type
memrec search "decision" --mtype decision

# Human-readable format
memrec search "architecture" --human
```

#### Similarity Scores

**MiniLM-L6-v2:**

| Score Range | Meaning |
|-------------|---------|
| 0.9+ | Highly relevant, exact match |
| 0.8-0.9 | Relevant, semantically similar |
| 0.75-0.8 | Somewhat relevant |
| < 0.75 | Likely irrelevant (filtered by default) |

**BGE-M3:**

| Score Range | Meaning |
|-------------|---------|
| 0.7+ | Highly relevant, exact match |
| 0.5-0.7 | Relevant, semantically similar |
| 0.4-0.5 | Somewhat relevant |
| < 0.4 | Likely irrelevant (filtered by default) |

**Adjusting the threshold:**

```bash
# Temporary
memrec search "query" --min-score 0.6

# Global (environment variable)
export MEMREC_MIN_SCORE=0.6
memrec search "query"
```

#### Output Format

**Default JSON (AI Agent friendly):**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "type": "semantic_search_result",
    "results": [
      {
        "memory_id": "550e8400-...",
        "score": 0.86,
        "memory_type": "decision",
        "content_preview": "Choose JWT auth...",
        "project_id": "312a9769-...",
        "tags": ["auth", "critical"],
        "created_at": "2026-04-23T..."
      }
    ],
    "total": 5,
    "query_embedding_time_ms": 2,
    "search_time_ms": 0
  }
}
```

**Human-readable (`--human`):**

```
Found 5 memories:

[DECISION] Choose JWT auth... (score: 0.86)
  ID: 550e8400-...
  Project: 312a9769-...
  Tags: ["auth", "critical"]
  Created: 2026-04-23
```

### Get Memory Details

```bash
# Get a single memory
memrec get <memory-id>

# Get full content of a chunked memory
memrec get <memory-id> --merge
```

### List Memories

```bash
memrec list                  # Default: 20 items
memrec list --limit 50       # List 50
memrec list -p               # Current project only (default when .mr_pid exists)
memrec list -g               # Global memories only (default without .mr_pid)
memrec list -a               # All projects
```

### Delete Memory

```bash
memrec delete <memory-id>
```

Deletion is soft — memories are marked as `is_deleted=true`.

### Statistics

```bash
memrec stats
```

### Dream Consolidation & Audit

Dream is triggered automatically when the system is idle, or manually. Consolidation uses a two-stage pipeline (Stage F fusion + Stage N networking); see the README "Dream Consolidation" section. Every run (including gate rejection, dry-run and failure) is persisted as a `DreamRun` audit record.

```bash
memrec dream --force              # trigger consolidation (bypasses time/new-memory gates)
memrec dream --dry-run            # dry-run: compute only, write nothing, delete nothing, build no edges
memrec dream-report --last 5      # inspect recent DreamRun records (candidates/clusters/edges/propagation/status)
```

### Offline Retrieval Evaluation

Drive read-only retrieval over a JSONL dataset and report `hit@k`/`recall@k`/`MRR`/`nDCG`, useful for comparing retrieval configurations (e.g. graph diffusion on/off).

```bash
memrec eval dataset.jsonl --k 10 --human
```

Dataset line format: `{"query": "...", "relevant": ["<uuid>", ...]}`.

## Project Isolation

### Auto-Detection

MemRec automatically detects project context:

1. **Git repos**: Auto-detect git root (`git rev-parse --show-toplevel`)
2. **Non-git dirs**: Use current working directory
3. **Project ID**: Created in `.mr_pid` file at project root

### .mr_pid File

Auto-created, no manual management needed:

```
your-project/
├── .mr_pid               # Project ID file
├── .gitignore            # Add .mr_pid to .gitignore
└── ...
```

Contents:

```
memrec_project_id=b435a636-481b-43dd-a819-cc2cedebf365
created_at=2026-04-23T02:45:47.828915366+00:00
```

**Notes:**

- Add `.mr_pid` to `.gitignore` to avoid project ID conflicts in team collaboration
- Moving a project directory preserves the project_id (`.mr_pid` moves with the project)
- All subdirectories within a git repo share the same project_id

### Isolation Example

```bash
# In memrec project
cd /disk2/code/rust/memrec
memrec add "memrec architecture: Rust+RocksDB+UnixSocket" --mtype context
# → Written to memrec/.mr_pid's project_id

# In hydrakiller project
cd /disk2/code/java/hydrakiller
memrec add "hydrakiller tech stack: Kotlin+Spring+ONNX" --mtype context
# → Written to hydrakiller/.mr_pid's project_id (different)

# Search auto-isolates
cd /disk2/code/rust/memrec
memrec search "architecture" -p          # Only memrec project results

# Cross-project search
memrec search "architecture" -a           # All projects' architecture memories
```

## Configuration

### config.toml

Location: `~/.memrec/config.toml`

**MiniLM-L6-v2 config:**

```toml
version = "0.3.0"

[model]
model_type = "minilm-l6-v2"
source = "huggingface"
dimension = 384

[[model.files]]
filename = "model.onnx"
remote_path = "model.onnx"
sha256 = "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"
file_type = "onnx-model"
required = true

[[model.files]]
filename = "tokenizer.json"
remote_path = "tokenizer.json"
sha256 = "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0"
file_type = "tokenizer"
required = true

# ... more files

[server]
socket_path = "~/.memrec/memrecd.sock"
data_dir = "~/.memrec/data"
vectors_dir = "~/.memrec/vectors"
log_file = "~/.memrec/memrecd.log"
```

**BGE-M3 config:**

```toml
version = "0.3.0"

[model]
model_type = "bge-m3"
source = "huggingface"
dimension = 1024

[[model.files]]
filename = "model.onnx"
remote_path = "onnx/model.onnx"
sha256 = "f84251230831afb359ab26d9fd37d5936d4d9bb5d1d5410e66442f630f24435b"
file_type = "onnx-model"
required = true

[[model.files]]
filename = "model.onnx_data"
remote_path = "onnx/model.onnx_data"
sha256 = "1eebfb28493f67bba03ce0ef64bfdc7fc5a3bd9d7493f818bb1d78cd798416b4"
file_type = "onnx-external-data"
required = true

# ... more files

[server]
socket_path = "~/.memrec/memrecd.sock"
data_dir = "~/.memrec/data"
vectors_dir = "~/.memrec/vectors"
log_file = "~/.memrec/memrecd.log"
```

### Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `MEMREC_MODEL_DIR` | Custom model path | `~/.memrec/models/<model-dir>/` |
| `MEMREC_MIN_SCORE` | Min similarity score | 0.75 (MiniLM) / 0.5 (BGE-M3) |
| `MEMREC_LLM_KEY` | LLM API key (`[llm] api_key` may reference `"$MEMREC_LLM_KEY"`; systemd service loads it from `~/.memrec/memrecd.env`) | empty |
| `RUST_LOG` | Log level | `info` |

### LLM & Dream Configuration

Optionally connect an OpenAI-compatible LLM to power Dream consolidation summaries; `[dream]` controls consolidation behavior.

```toml
[llm]
enabled = false            # enable to unlock Dream summaries
provider = "deepseek"       # deepseek / openai / custom (any OpenAI-compatible)
url = "https://api.deepseek.com/v1"
api_key = ""                # supports env var reference (e.g. "$MEMREC_LLM_KEY")
model = "deepseek-chat"
think_level = "medium"      # off / minimal / low / medium / high / xhigh
timeout_secs = 120
max_tokens = 4096
temperature = 0.3

[dream]
enabled = false
requires_llm = true
min_memories = 20           # minimum consolidatable memories
max_age_hours = 168         # max age of memories to consolidate (hours)
min_hours_between = 24.0    # min interval between runs (hours)
min_new_memories = 1        # user-added memory gate since last run
integration_max_memories = 100        # input cap per run (ascending importance, avoids token overflow)
integration_preserve_importance = 0.85  # preserve high-importance memories
integration_preserve_tags = ["critical"]  # preserve matching tags
integration_preserve_types = ["preference", "context"]  # preserved types (as-is; context details are irrecoverable)
integration_min_content_len = 10      # filter short test/placeholder records (chars)

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

[dedup]                     # write dedup (ADD/UPDATE/NOOP)
enabled = true
top_k = 5
duplicate_threshold = 0.92  # above this = duplicate, skip
update_threshold = 0.85     # above this = update existing
update_window_mins = 60     # in-place update window (minutes); outside it a new record is added and the old one marked superseded; 0 = unlimited

[log]
enabled = true             # file logging (stdout only when false)
level = "info"             # trace/debug/info/warn/error
rotation_days = 7          # keep N daily files (rotated at local midnight)
memrecd_log = "~/.memrec/logs/memrecd.log"  # full log path (local-timezone timestamps, e.g. +08:00)
```

> The systemd user service does not inherit shell env vars: write the API key to `~/.memrec/memrecd.env` (mode 600, loaded by the service template via `EnvironmentFile`) and run `systemctl --user restart memrecd`; when starting manually in a terminal, just `export` it.

## Data Directory

```
~/.memrec/
├── config.toml           # Configuration
├── memrecd.sock          # Unix Socket (runtime)
├── memrecd.log           # Service log
├── data/                 # RocksDB memory metadata
├── vectors/              # RocksDB vector storage
├── models/               # ONNX Embedding models
│   ├── Qdrant--all-MiniLM-L6-v2-onnx/   # MiniLM-L6-v2
│   │   ├── model.onnx
│   │   ├── tokenizer.json
│   │   ├── config.json
│   │   ├── special_tokens_map.json
│   │   └── tokenizer_config.json
│   └── BAAI--bge-m3/                     # BGE-M3
│       ├── model.onnx
│       ├── model.onnx_data
│       ├── Constant_7_attr__value
│       ├── tokenizer.json
│       ├── config.json
│       ├── special_tokens_map.json
│       ├── tokenizer_config.json
│       └── sentencepiece.bpe.model
└── logs/                 # Log directory
```

## Switching Models

To switch from MiniLM to BGE-M3 (or vice versa):

1. **Reinstall with new model:**
   ```bash
   mr-install --model bge-m3 --skip-install --skip-service
   ```

2. **Back up existing vectors:**
   ```bash
   mv ~/.memrec/vectors ~/.memrec/vectors.minilm.bak
   ```

3. **Restart the daemon** (vectors will be auto-rebuilt):
   ```bash
   rm -f ~/.memrec/memrecd.sock
   systemctl --user restart memrecd
   ```

4. **Wait for rebuild** (~75-90s for BGE-M3 with ~500 memories)

**Important:** Switching models requires rebuilding all vectors because different models produce different-dimensional embeddings. Old vectors are incompatible with the new model.

## MCP Server

MemRec supports the Model Context Protocol (MCP) for direct AI client integration:

```bash
memrec --mcp    # Start MCP server in stdio mode
```

### MCP Tools

| Tool | Purpose |
|------|---------|
| `mr_add` | Add a memory |
| `mr_search` | Semantic search |
| `mr_get` | Get memory details |
| `mr_list` | List memories |
| `mr_delete` | Delete a memory |
| `mr_stats` | Get statistics |
| `mr_dream` | Trigger Dream consolidation (`force`, `dry_run`) |
| `mr_dream_report` | Inspect recent DreamRun audit records |

### MCP Resources

| Resource | Purpose |
|----------|---------|
| `memrec://stats` | System statistics |
| `memrec://project` | Current project info |

## AI Agent Workflow

```bash
# 1. Start task: retrieve relevant history
memrec search "related topic" -p

# 2. After making a decision: record it
memrec add "Choose X approach, reason: ..." --mtype decision --tag critical

# 3. User expresses preference: record as global
memrec add "User prefers Y" --mtype preference --global

# 4. Discover cross-project connections
memrec search "related topic" -a

# 5. End task
memrec stats
```

### Skill Integration

AI CLI tools (like opencode) can integrate via Skill:

- Skill file: `~/.opencode/skills/memrec/SKILL.md`
- AI agent auto-reads Skill to learn commands and best practices

## Performance

| Metric | MiniLM-L6-v2 | BGE-M3 |
|--------|-------------|--------|
| Startup time | < 50ms | ~75-90s (model load + vector rebuild) |
| Request latency | < 1ms | < 1ms |
| Search latency | < 5ms | < 10ms |
| Memory usage | ~118MB | ~1.5GB |
| Vector dimension | 384 | 1024 |
| Model disk size | ~90MB | ~2.3GB |

## Troubleshooting

### "Failed to connect to memrecd"

Daemon not running. Start it:

```bash
memrecd
# or
systemctl --user start memrecd
```

### Search returns 0 results

1. Check model files: `ls ~/.memrec/models/`
2. Check service log: `cat ~/.memrec/memrecd.log`
3. Lower min_score: `memrec search "query" --min-score 0.3`
4. For BGE-M3, the default min_score is 0.5 — try lowering to 0.3-0.4

### Project memories not isolated

1. Confirm `.mr_pid` exists in project root
2. Confirm you're running commands inside the project directory (not `~` or `/tmp`)
3. Git repos auto-detect git root

### Model download fails (China)

```bash
mr-install --use-hf-mirror
```

### Switching models

1. Reinstall with new model: `mr-install --model <model> --skip-install --skip-service`
2. Back up vectors: `mv ~/.memrec/vectors ~/.memrec/vectors.bak`
3. Restart daemon: `rm -f ~/.memrec/memrecd.sock && systemctl --user restart memrecd`

## Upgrade

```bash
mr-install
```

Re-running mr-install upgrades everything (cargo install overwrites old versions, service restarts).

## Uninstall

```bash
# Linux
systemctl --user stop memrecd
systemctl --user disable memrecd
rm ~/.config/systemd/user/memrecd.service
systemctl --user daemon-reload
rm ~/.local/bin/memrec ~/.local/bin/memrecd ~/.local/bin/mr-install

# macOS
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/cn.itcraft.memrecd.plist
rm ~/Library/LaunchAgents/cn.itcraft.memrecd.plist
rm ~/bin/memrec ~/bin/memrecd ~/bin/mr-install

# Delete data (optional, clears all memories)
rm -rf ~/.memrec
```
