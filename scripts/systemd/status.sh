#!/bin/bash
# Quick status check for memrecd

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "MemRec Status"
echo "============="

# Check systemd service
if systemctl --user is-active memrecd.service >/dev/null 2>&1; then
    echo -e "Service: ${GREEN}Running${NC}"
elif systemctl --user is-enabled memrecd.service >/dev/null 2>&1; then
    echo -e "Service: ${YELLOW}Enabled (stopped)${NC}"
else
    echo -e "Service: ${RED}Not installed${NC}"
fi

# Check socket
SOCKET="$HOME/.memrec/memrecd.sock"
if [ -S "$SOCKET" ]; then
    echo -e "Socket:  ${GREEN}Ready${NC} ($SOCKET)"
else
    echo -e "Socket:  ${RED}Not found${NC}"
fi

# Check binary
BINARY="$HOME/.local/bin/memrecd"
if [ -f "$BINARY" ]; then
    VERSION=$($BINARY --version 2>/dev/null || echo "unknown")
    echo -e "Binary:  ${GREEN}Installed${NC} ($BINARY, v$VERSION)"
else
    echo -e "Binary:  ${RED}Not found${NC}"
fi

# Show data stats
DATA_DIR="$HOME/.memrec/data"
if [ -d "$DATA_DIR" ]; then
    SIZE=$(du -sh "$DATA_DIR" | cut -f1)
    echo -e "Data:    ${GREEN}$SIZE${NC} ($DATA_DIR)"
else
    echo -e "Data:    ${YELLOW}Not initialized${NC}"
fi

# Show recent logs
LOG_FILE="$HOME/.memrec/memrecd.log"
if [ -f "$LOG_FILE" ]; then
    echo ""
    echo "Recent logs (last 5 lines):"
    tail -5 "$LOG_FILE" | sed 's/^/  /'
fi