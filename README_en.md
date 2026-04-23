[中文版](README_cn.md) | English

# MemRec - AI Memory Persistence System

Local memory persistence system for AI CLI tools (opencode, claude code, etc.), providing cross-session memory recovery, knowledge accumulation, and conversation archival capabilities.

## Features

- **Cross-session recovery** - Restore context, preferences, project knowledge
- **Knowledge accumulation** - Store best practices and key decisions
- **Conversation archival** - Complete conversation history with retrieval
- **Smart lifecycle management** - Automatic compression and forgetting based on importance scoring
- **Hybrid search** - Exact + semantic retrieval with RRF fusion
- **Auto-splitting** - Long content (>7.5KB) automatically split into chunks
- **Systemd integration** - Easy install with `systemctl --user`

## Quick Start

### Install

```bash
# Build release binaries
cargo build --release

# Install binaries
install -m 755 target/release/memrecd ~/.local/bin/
install -m 755 target/release/memrec ~/.local/bin/

# Install systemd service (optional)
./scripts/systemd/install.sh
```

### Usage

```bash
# Start daemon (if not using systemd)
memrecd

# Add memories
memrec add "Key decision" --mtype decision --tag critical
memrec add "Best practice" --mtype knowledge --tag rust
memrec add "Project config" --mtype context --tag config

# Retrieve memories
memrec list --limit 20
memrec get <id>
memrec stats

# Delete memories
memrec delete <id>
```

## Memory Types

- `decision` - Key decisions (recommended: use `critical` tag)
- `knowledge` - Best practices and learnings
- `context` - Project configuration and environment
- `preference` - User preferences
- `conversation` - Conversation records (default)

## Memory Management

Automatic lifecycle management:
- **Importance scoring**: Time decay + access frequency + tag weights + user priority
- **Compression**: Low importance memories compressed to summaries
- **Forgetting**: importance < 0.1 and inactive > 90 days → deletion

## Service Management

Two management approaches available:

### Option 1: Manual Scripts (Recommended for Development)

```bash
# Start
./scripts/start.sh

# Stop
./scripts/stop.sh

# Restart
./scripts/restart.sh

# Status
./scripts/status.sh
```

Features:
- PID file management
- Background execution
- Log output to `~/.memrec/memrecd.log`
- Graceful shutdown (SIGTERM, force after 10s timeout)

### Option 2: Systemd Service (Recommended for Production)

```bash
# Install
./scripts/systemd/install.sh

# Manage
systemctl --user start memrecd
systemctl --user stop memrecd
systemctl --user status memrecd
journalctl --user -u memrecd -f
```

Or use the convenience script:

```bash
./scripts/memrecctl.sh start
./scripts/memrecctl.sh stop
./scripts/memrecctl.sh status
./scripts/memrecctl.sh logs
```

## Skill Integration

Skill for AI CLI tools: `~/.opencode/skills/memrec/SKILL.md`

AI agents can:
- Record key decisions automatically
- Retrieve historical knowledge
- Maintain project context across sessions
- Remember user preferences

## Documentation

- [Systemd Guide](docs/systemd.md)
- [Design Spec](docs/superpowers/specs/2026-04-23-memrec-design.md)
- [Algorithms](docs/superpowers/specs/2026-04-23-memrec-algorithms.md)
- [Skill Documentation](docs/skills/memrec-skill.md)

## Project Structure

```
memrec/
├── common/       # Shared types and protocols
├── memrecd/      # Daemon service
├── memrec/       # CLI tool
└── docs/         # Documentation
```

## License

MIT

## Changelog

See [CHANGELOG.md](CHANGELOG.md)