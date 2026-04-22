#!/bin/bash
set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${RED}MemRec Systemd Service Uninstaller${NC}"
echo ""

read -p "This will stop and remove memrecd service. Continue? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    exit 1
fi

# Stop service
echo "Stopping memrecd..."
systemctl --user stop memrecd.service || true

# Disable service
echo "Disabling memrecd..."
systemctl --user disable memrecd.service || true

# Remove service file
echo "Removing service file..."
rm -f "$HOME/.config/systemd/user/memrecd.service"

# Reload systemd
echo "Reloading systemd daemon..."
systemctl --user daemon-reload

read -p "Remove memrecd binary? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    rm -f "$HOME/.local/bin/memrecd"
    rm -f "$HOME/.local/bin/memrec"
    echo "Binaries removed."
fi

read -p "Remove data directory (~/.memrec)? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    rm -rf "$HOME/.memrec"
    echo "Data directory removed."
fi

echo ""
echo -e "${GREEN}Uninstallation complete!${NC}"