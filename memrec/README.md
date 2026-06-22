# MemRec CLI — AI Memory Persistence Client

[![Crates.io](https://img.shields.io/crates/v/memrec.svg)](https://crates.io/crates/memrec)
[![Documentation](https://docs.rs/memrec/badge.svg)](https://docs.rs/memrec)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Command-line interface for the MemRec AI memory persistence system, providing intuitive access to memory storage, semantic search, and project management.

## Overview

`memrec` is the primary CLI client for interacting with the MemRec daemon. It provides a comprehensive set of commands for managing AI memories with project isolation, semantic search capabilities, and seamless integration with AI tools and workflows.

## Features

- **Project-Aware**: Automatically detects project context via `.mr_pid` files
- **Semantic Search**: Find memories by meaning, not just keywords
- **Memory Management**: Add, retrieve, update, and delete memories
- **Importance Scoring**: Automatic relevance ranking with manual adjustment
- **Tag System**: Flexible tagging for organization and discovery
- **JSON Output**: AI-friendly default output format with `--human` flag
- **Batch Operations**: Support for processing multiple memories
- **MCP Support**: Model Context Protocol integration for AI tools

## Installation

### From crates.io (Recommended)

```bash
cargo install --locked memrec
```

### Using mr-install (All-in-one)

```bash
cargo install --locked mr-install
mr-install
```

## Quick Start

```bash
# First, ensure the daemon is running
memrecd &

# Add your first memory (project context auto-detected)
memrec add meeting-notes --mtype conversation \
  --content "Discussed project architecture and decided on microservices" \
  --tags architecture meeting planning

# Search for related memories
memrec search "microservices architecture"

# List all memories in current project
memrec list

# Get specific memory
memrec get meeting-notes
```

## Usage

### Basic Commands

#### Memory Operations

```bash
# Add a new memory
memrec add <id> --mtype <type> --content <text> [--tags <tag1,tag2>] [--importance <0.0-1.0>]

# Examples:
memrec add api-design --mtype code --content "API design patterns for REST services" --tags api design patterns
memrec add bug-fix --mtype fix --content "Fixed memory leak in vector store" --tags bugfix performance --importance 0.9

# Get a memory by ID
memrec get <id>

# Update a memory
memrec update <id> --content <new-text> [--tags <new-tags>]

# Delete a memory (soft delete)
memrec delete <id>

# List memories with pagination
memrec list [--limit <n>] [--offset <n>] [--order-by <field>] [--asc|--desc]
```

#### Search Operations

```bash
# Semantic search (vector similarity)
memrec search <query> [--limit <n>] [--min-score <0.0-1.0>]

# Search with tags
memrec search --tags <tag1,tag2> [--limit <n>]

# Combined search
memrec search "database optimization" --tags performance sql --limit 10

# Search across all projects
memrec search --global "common patterns"
```

#### Project Operations

```bash
# Get current project info
memrec project

# Manually set project
memrec project set <project-id>

# List all projects
memrec project list

# Create new project
memrec project create <name> [--description <text>] [--tags <tag1,tag2>]
```

### Advanced Usage

#### Import/Export

```bash
# Import from JSON file
memrec import --file memories.json [--project <project-id>]

# Export memories to JSON
memrec export --file backup.json [--project <project-id>]

# Export with filters
memrec export --file important.json --min-importance 0.7 --tags critical
```

#### Batch Operations

```bash
# Batch add from stdin (JSON lines)
cat memories.jsonl | memrec batch add

# Batch update
echo '{"id": "mem1", "content": "updated"}' | memrec batch update

# Batch delete
echo '["mem1", "mem2"]' | memrec batch delete
```

#### Importance Management

```bash
# Recalculate importance for all memories
memrec importance recalc

# Get importance statistics
memrec importance stats

# Set manual importance override
memrec set-importance <id> <0.0-1.0>
```

## Output Formats

### JSON (Default for AI tools)

```bash
# Default JSON output
memrec get meeting-notes
# Output: {"id": "meeting-notes", "content": "...", "importance": 0.85, ...}

# Pretty JSON
memrec get meeting-notes --json-pretty
```

### Human-Readable

```bash
# Human-readable table format
memrec list --human

# Human-readable with specific columns
memrec list --human --columns id,content,importance,tags

# Colorized output
memrec search "patterns" --human --color
```

### Machine Formats

```bash
# CSV output
memrec list --format csv

# TSV output  
memrec list --format tsv

# YAML output
memrec get meeting-notes --format yaml
```

## Project Detection

MemRec automatically detects project context:

1. **`.mr_pid` File**: Looks for `.mr_pid` in current or parent directories
2. **Git Repository**: Falls back to git repository root as project
3. **Home Directory**: Uses `~/.memrec` for global memories
4. **Manual Override**: Use `memrec project set <id>` to override

Create a project identifier:

```bash
# In your project root
echo "my-awesome-project" > .mr_pid
# Now all memrec commands will use this project context
```

## Integration Examples

### With AI CLI Tools

```bash
# Store AI conversation history
ai --model gpt-4 "Explain microservices" | \
  memrec add ai-explanation --mtype conversation \
  --content "$(cat)" --tags ai microservices explanation

# Search for relevant context before asking AI
context=$(memrec search "database schema" --limit 3 --format content-only)
ai --model claude "Design a database schema. Context: $context"
```

### In Shell Scripts

```bash
#!/bin/bash
# Store command output as memory
memrec add "cmd-output-$(date +%s)" --mtype log \
  --content "$(some-command 2>&1)" \
  --tags script $(basename $0)

# Search for troubleshooting info
if memrec search "error $(some-command)" --min-score 0.8; then
  echo "Found similar error in memory"
fi
```

### With MCP (Model Context Protocol)

```bash
# MCP server provides memories as context to AI models
memrec mcp-server

# In your AI tool configuration:
# {
#   "mcpServers": {
#     "memrec": {
#       "command": "memrec",
#       "args": ["mcp-server"]
#     }
#   }
# }
```

## Configuration

### Environment Variables

```bash
# Socket path override
export MEMREC_SOCKET_PATH="/custom/path/memrecd.sock"

# Minimum search score
export MEMREC_MIN_SCORE=0.75

# Default output format
export MEMREC_OUTPUT_FORMAT="human"

# Model directory
export MEMREC_MODEL_DIR="$HOME/.memrec/models"

# Log level
export RUST_LOG="info"
```

### Configuration File

Create `~/.memrec/cli_config.toml`:

```toml
[defaults]
output_format = "json"  # or "human"
color = true
confirm_deletes = true

[search]
default_limit = 10
min_score = 0.75
include_global = false

[project]
auto_detect = true
fallback_to_git = true

[formatting]
date_format = "%Y-%m-%d %H:%M:%S"
truncate_content = 200
```

## Performance Tips

### Memory Usage
- Use concise but descriptive content for better embeddings
- Add relevant tags for improved search accuracy
- Set appropriate importance scores for frequently accessed memories

### Search Optimization
- Use specific queries rather than generic terms
- Combine semantic search with tag filters
- Adjust `--min-score` based on use case (0.75 default)

### Storage Management
- Regularly clean up low-importance memories
- Use `memrec importance recalc` to maintain accuracy
- Export important memories before cleanup

## Troubleshooting

### Common Issues

```bash
# Daemon not running
Error: Failed to connect to socket
Solution: Start the daemon with `memrecd` or check service status

# Project not detected  
Warning: No project context detected
Solution: Create `.mr_pid` file or use `memrec project set`

# Search returns no results
# Try: Adjust --min-score, add more specific terms, or check embedding model

# Permission denied
Error: Permission denied (os error 13)
Solution: Check socket permissions or run as correct user
```

### Debug Mode

```bash
# Enable debug output
RUST_LOG=debug memrec <command>

# Trace all operations
RUST_LOG=trace memrec <command> --verbose
```

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for development guidelines.

## License

Apache License 2.0 - see [LICENSE](../LICENSE) for details.

## Links

- [Main Repository](https://github.com/itcraft-cn/memrec)
- [API Documentation](https://docs.rs/memrec)
- [Crates.io](https://crates.io/crates/memrec)
- [Daemon Server](../memrecd/README.md)
- [Installer](../mr-install/README.md)