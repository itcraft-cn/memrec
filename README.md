[中文版](README_cn.md) | English

# MemRec — AI Memory Persistence System

> Local-first AI memory with project isolation — for terminal, for private use

Local memory persistence system for AI CLI tools, providing cross-session memory recovery, knowledge accumulation, and conversation archival.

## Features

- **Project Isolation** — Auto-detect git root, .mr_pid persistence, independent memory per project
- **Semantic Search** — Local ONNX model (384-dim), zero API cost, all data stays local
- **Cross-Project Search** — `--all` flag to discover related knowledge across projects
- **AI-first Design** — JSON output by default, concise commands, Skill integration
- **High Performance** — Rust, <1ms latency, ~118MB memory (including model)
- **Auto-splitting** — Long content (>7.5KB) automatically split into chunks
- **Systemd Integration** — Manage daemon with `systemctl --user`

## Quick Start

### Install

```bash
# Build
cargo build --release
cargo install --path memrec --locked
cargo install --path memrecd --locked
cargo install --path mr-install --locked

# Copy to system path (Linux: ~/.local/bin/, macOS: ~/bin/)
cp ~/.cargo/bin/memrec ~/.local/bin/
cp ~/.cargo/bin/memrecd ~/.local/bin/
cp ~/.cargo/bin/mr-install ~/.local/bin/

# One-click setup: directories, model, service, verification
mr-install
```

| Platform | Binary Path | Data Path |
|----------|------------|-----------|
| Linux | `~/.local/bin/` | `~/.memrec/` |
| macOS | `~/bin/` | `~/.memrec/` |
| Windows | `%APPDATA%\memrec\` | `~/.memrec/` |

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

# Semantic search
memrec search "auth"                        # Current project + global
memrec search "performance" --project-only  # Current project only
memrec search "preferences" --global-only   # Global memories only
memrec search "xlsb" --all                  # Across all projects

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
```

## Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `MEMREC_MODEL_DIR` | Custom model path | `~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx/` |
| `MEMREC_MIN_SCORE` | Min similarity score | `0.75` |
| `RUST_LOG` | Log level | `info` |

## Documentation

- [Installation Guide](docs/installation.md)
- [User Guide](docs/user-guide.md)
- [Skill Documentation](docs/skills/memrec-skill.md)

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
