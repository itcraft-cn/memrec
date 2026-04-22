#!/bin/bash
set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}MemRec Systemd Service Installer${NC}"
echo ""

# Check if memrecd binary exists
MEMRECD_PATH="$HOME/.local/bin/memrecd"

if [ ! -f "$MEMRECD_PATH" ]; then
    echo -e "${YELLOW}Warning: memrecd not found at $MEMRECD_PATH${NC}"
    echo "Please build and install memrecd first:"
    echo "  cargo build --release"
    echo "  install -m 755 target/release/memrecd ~/.local/bin/"
    echo ""
    read -p "Continue anyway? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

# Create directories
echo "Creating directories..."
mkdir -p "$HOME/.config/systemd/user"
mkdir -p "$HOME/.local/bin"
mkdir -p "$HOME/.memrec/data"

# Install memrecd binary
if [ -f "./target/release/memrecd" ]; then
    echo "Installing memrecd binary..."
    install -m 755 ./target/release/memrecd "$HOME/.local/bin/"
fi

if [ -f "./target/release/memrec" ]; then
    echo "Installing memrec CLI..."
    install -m 755 ./target/release/memrec "$HOME/.local/bin/"
fi

# Copy systemd service file
echo "Installing systemd service file..."
sed -e "s|%h|$HOME|g" scripts/systemd/memrecd.service > "$HOME/.config/systemd/user/memrecd.service"

# Reload systemd
echo "Reloading systemd daemon..."
systemctl --user daemon-reload

# Enable service
echo "Enabling memrecd service..."
systemctl --user enable memrecd.service

# Ask to start now
echo ""
read -p "Start memrecd now? (Y/n) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Nn]$ ]]; then
    echo "Starting memrecd..."
    systemctl --user start memrecd.service
    sleep 2
    systemctl --user status memrecd.service --no-pager || true
fi

echo ""
echo -e "${GREEN}Installation complete!${NC}"
echo ""
echo "Usage:"
echo "  systemctl --user start memrecd    # Start daemon"
echo "  systemctl --user stop memrecd     # Stop daemon"
echo "  systemctl --user status memrecd   # Check status"
echo "  systemctl --user restart memrecd  # Restart daemon"
echo "  systemctl --user logs memrecd     # View logs (or: journalctl --user -u memrecd)"
echo ""
echo "The daemon will auto-start on login."
echo "Logs: $HOME/.memrec/memrecd.log"