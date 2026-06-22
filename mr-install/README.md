# MemRec Installer — One-Stop Installation Tool

[![Crates.io](https://img.shields.io/crates/v/mr-install.svg)](https://crates.io/crates/mr-install)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

One-stop installation tool for the MemRec AI memory persistence system, providing secure, platform-specific installation with automatic service configuration.

## Overview

`mr-install` is the recommended way to install the complete MemRec ecosystem. It handles everything from downloading dependencies, installing binaries, setting up services, and configuring the system for optimal performance and security.

## Features

- **One-Command Installation**: Complete setup with a single command
- **Platform Support**: Linux (systemd) and macOS (launchd) services
- **Security Hardened**: SHA256 hash verification, Git repository whitelist
- **Automatic Service Management**: Background daemon setup with auto-restart
- **Model Download**: Automatic embedding model download with fallback mirrors
- **Configuration Generation**: Automatic `~/.memrec/config.toml` creation
- **Verification Tests**: Post-install verification to ensure everything works
- **Uninstall Support**: Clean removal of all components

## Quick Start

```bash
# Install mr-install from crates.io
cargo install --locked mr-install

# Run the installer (recommended for first-time setup)
mr-install

# Alternative: Install directly without separate step
cargo install --locked mr-install && mr-install
```

## Installation Methods

### Standard Installation (Recommended)

```bash
mr-install
```

This performs:
1. ✅ Binary installation via `cargo install`
2. ✅ Model download (~90MB for MiniLM-L6-v2)
3. ✅ Service registration (systemd/launchd)
4. ✅ Configuration generation
5. ✅ Verification tests

### Custom Installation Options

```bash
# Use HuggingFace mirror (for China users)
mr-install --use-hf-mirror

# Custom Git repository (advanced users)
mr-install --repo-url "https://gitee.com/itcraft-cn/memrec"

# Skip verification tests (development)
mr-install --skip-verify

# Skip hash verification (security risk - not recommended)
mr-install --skip-hash-verify

# Allow any Git repository (security risk - development only)
mr-install --allow-any-repo --repo-url "https://example.com/custom-repo"
```

### Platform-Specific Details

#### Linux (systemd)

```bash
# Installation directories
~/.local/bin/memrec        # CLI client
~/.local/bin/memrecd       # Daemon server
~/.local/bin/mr-install    # Installer itself

# Service file
~/.config/systemd/user/memrecd.service

# Manual service control
systemctl --user status memrecd
systemctl --user start memrecd
systemctl --user stop memrecd
systemctl --user enable memrecd   # Auto-start on login
```

#### macOS (launchd)

```bash
# Installation directories
~/bin/memrec               # CLI client
~/bin/memrecd              # Daemon server
~/bin/mr-install           # Installer itself

# Service file
~/Library/LaunchAgents/com.itcraft.memrecd.plist

# Manual service control
launchctl list com.itcraft.memrecd
launchctl start com.itcraft.memrecd
launchctl stop com.itcraft.memrecd
launchctl bootstrap gui/$UID ~/Library/LaunchAgents/com.itcraft.memrecd.plist
```

## Security Features

### Model Integrity Verification

```bash
# SHA256 hash verification for all model files
const MODEL_HASHES: &[(&str, &str)] = &[
    ("model.onnx", "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"),
    ("tokenizer.json", "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0"),
    # ... more files
];

# Verification process:
# 1. Download file
# 2. Calculate SHA256 hash
# 3. Compare with expected hash
# 4. Re-download if mismatch
# 5. Skip only with explicit --skip-hash-verify
```

### Git Repository Whitelist

```rust
const ALLOWED_GIT_REPOS: &[&str] = &[
    "https://github.com/itcraft-cn/memrec",    # Official
    "https://gitee.com/itcraft-cn/memrec",     # Mirror
];

// Only these repositories are allowed by default
// Use --allow-any-repo to bypass (security risk)
```

### Download Sources and Fallbacks

```bash
# Primary source (default)
https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx

# Automatic fallback (if primary fails)
https://hf-mirror.com/Qdrant/all-MiniLM-L6-v2-onnx

# Custom mirror
mr-install --mirror-base-url "https://custom-mirror.example.com"
```

## Installation Process Details

### Step 1: Binary Installation

```bash
# Installs via cargo install --locked
cargo install --locked memrec-common
cargo install --locked memrecd
cargo install --locked memrec
cargo install --locked mr-install

# Options:
# --repo-url: Custom Git repository (whitelist enforced)
# --allow-any-repo: Disable whitelist (security risk)
```

### Step 2: Directory Setup

```bash
# Creates necessary directories
~/.memrec/
├── models/                    # Embedding models
│   └── Qdrant--all-MiniLM-L6-v2-onnx/
├── data/                      # Metadata storage (RocksDB)
├── vectors/                   # Vector storage (RocksDB)
├── config.toml                # Configuration file
└── memrecd.log               # Daemon log file
```

### Step 3: Model Download

Downloads the embedding model (~90MB) with:
- Progress bars for each file
- Hash verification (SHA256)
- Primary source + fallback mirror
- Resume capability for existing files

### Step 4: Service Registration

#### Linux (systemd)
```ini
[Unit]
Description=MemRec Memory Persistence Daemon
Documentation=https://github.com/itcraft-cn/memrec
After=default.target

[Service]
Type=simple
ExecStart=/home/user/.local/bin/memrecd
ExecStopPost=/bin/rm -f /home/user/.memrec/memrecd.sock
Restart=on-failure
RestartSec=5
Environment="RUST_LOG=info"
WorkingDirectory=/home/user/.memrec
StandardOutput=append:/home/user/.memrec/memrecd.log
StandardError=append:/home/user/.memrec/memrecd.log

[Install]
WantedBy=default.target
```

#### macOS (launchd)
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.itcraft.memrecd</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/user/bin/memrecd</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>StandardOutPath</key>
    <string>/Users/user/.memrec/memrecd.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/user/.memrec/memrecd.log</string>
    <key>WorkingDirectory</key>
    <string>/Users/user/.memrec</string>
</dict>
</plist>
```

### Step 5: Configuration Generation

Creates `~/.memrec/config.toml`:

```toml
version = "0.3.0"

[model]
model_type = "minilm-l6-v2"
source = "huggingface"
dimension = 384

[[model.files]]
filename = "model.onnx"
sha256 = "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"
required = true

# ... more files with hashes

[server]
socket_path = "~/.memrec/memrecd.sock"
data_dir = "~/.memrec/data"
vector_dir = "~/.memrec/vectors"
log_path = "~/.memrec/memrecd.log"
```

### Step 6: Verification Tests

```bash
# Tests performed:
1. ✅ Check if binaries are executable
2. ✅ Verify socket connection
3. ✅ Test memory addition
4. ✅ Test memory retrieval
5. ✅ Test semantic search
```

## Troubleshooting

### Common Issues

#### Installation Fails

```bash
# Check Rust installation
rustc --version
cargo --version

# Check network connectivity
curl -I https://crates.io
curl -I https://huggingface.co

# Try with mirror
mr-install --use-hf-mirror

# Skip verification (development)
mr-install --skip-verify
```

#### Service Won't Start

```bash
# Check logs
cat ~/.memrec/memrecd.log

# Linux: Check systemd status
systemctl --user status memrecd
journalctl --user -u memrecd

# macOS: Check launchd
launchctl list com.itcraft.memrecd
log stream --predicate 'subsystem == "com.itcraft.memrecd"'

# Manual start
~/.local/bin/memrecd  # Linux
~/bin/memrecd         # macOS
```

#### Permission Issues

```bash
# Check directory permissions
ls -la ~/.memrec/
ls -la ~/.local/bin/  # Linux
ls -la ~/bin/         # macOS

# Check socket permissions
ls -la ~/.memrec/memrecd.sock

# Fix permissions (if needed)
chmod 755 ~/.memrec
chmod 600 ~/.memrec/config.toml
```

### Debug Mode

```bash
# Verbose output
RUST_LOG=debug mr-install

# Trace all operations
RUST_LOG=trace mr-install
```

## Uninstallation

### Manual Removal

```bash
# Stop and disable service
systemctl --user stop memrecd          # Linux
systemctl --user disable memrecd
launchctl stop com.itcraft.memrecd     # macOS
launchctl bootout gui/$UID ~/Library/LaunchAgents/com.itcraft.memrecd.plist

# Remove binaries
rm ~/.local/bin/memrec ~/.local/bin/memrecd ~/.local/bin/mr-install  # Linux
rm ~/bin/memrec ~/bin/memrecd ~/bin/mr-install                       # macOS

# Remove data and configuration
rm -rf ~/.memrec

# Remove service files
rm ~/.config/systemd/user/memrecd.service                            # Linux
rm ~/Library/LaunchAgents/com.itcraft.memrecd.plist                  # macOS
```

### Using Package Manager

```bash
# Remove via cargo
cargo uninstall memrec memrecd memrec-common mr-install
```

## Development

### Building from Source

```bash
git clone https://github.com/itcraft-cn/memrec
cd memrec/mr-install

# Build
cargo build --release

# Test
cargo test --release

# Install locally
cargo install --path .
```

### Adding New Platforms

To add support for new platforms, implement the `ServiceManager` trait:

```rust
pub trait ServiceManager {
    fn name(&self) -> &str;
    fn register(&self, bin_path: &Path, home_dir: &Path) -> Result<()>;
    fn start(&self) -> Result<()>;
    fn stop(&self) -> Result<()>;
    fn is_active(&self) -> bool;
    fn unregister(&self) -> Result<()>;
}
```

## Security Considerations

### Risk Assessment

| Risk | Mitigation | Default |
|------|------------|---------|
| Malicious model injection | SHA256 hash verification | Enabled |
| Untrusted Git repository | Whitelist enforcement | Enabled |
| Service command injection | Hardcoded service files | Enabled |
| Permission escalation | User-mode service | Enabled |

### Recommended Practices

1. **Always verify hashes** unless absolutely necessary
2. **Use official repositories** when possible
3. **Review service files** after installation
4. **Monitor logs** for unusual activity
5. **Regular updates** to get security fixes

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for development guidelines.

## License

Apache License 2.0 - see [LICENSE](../LICENSE) for details.

## Links

- [Main Repository](https://github.com/itcraft-cn/memrec)
- [Crates.io](https://crates.io/crates/mr-install)
- [CLI Client](../memrec/README.md)
- [Daemon Server](../memrecd/README.md)
- [Security Analysis](../SECURITY_ANALYSIS.md)