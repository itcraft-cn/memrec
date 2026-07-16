[中文版](README_cn.md) | English

# MemRec — AI Memory Persistence System

> Local-first AI memory with project isolation — for terminal, for private use

Local memory persistence system for AI CLI tools, providing cross-session memory recovery, knowledge accumulation, and conversation archival.

## Features

- **Project Isolation** — Auto-detect git root, .mr_pid persistence, independent memory per project
- **Hybrid Search** — KNN vector search + BM25 full-text, MMR reranking, Chinese text support
- **Semantic Search** — Local ONNX model — MiniLM-L6-v2 (384d) or BGE-M3 (1024d, multilingual), zero API cost
- **Smart Scoring** — Time decay, evergreen exemption, source weighting
- **Cross-Project Search** — `--all` flag to discover related knowledge across projects
- **AI-first Design** — JSON output by default, concise commands, Skill integration
- **High Performance** — Rust, <1ms latency, ~118MB (MiniLM) / ~1.5GB (BGE-M3) memory (including model)
- **Auto-splitting** — Long content (>7.5KB) automatically split into chunks
- **Systemd Integration** — Manage daemon with `systemctl --user`

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
memrec get <id>
memrec stats
memrec version
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

## Data Location

```
~/.memrec/
├── config.toml           # Configuration
├── memrecd.sock          # Unix Socket
├── data/                 # RocksDB memory metadata
├── vectors/              # RocksDB vector storage
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

## Project Structure

```
memrec/
├── common/       # Shared types and protocol
├── memrecd/      # Daemon service
├── memrec/       # CLI tool
├── mr-install/   # Installer
└── docs/         # Documentation
```

## License

Apache-2.0

## Changelog

See [CHANGELOG.md](CHANGELOG.md)
