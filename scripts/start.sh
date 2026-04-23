#!/bin/bash
# Start memrecd daemon manually (without systemd)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

MEMRECD_BIN="${MEMRECD_BIN:-$HOME/.local/bin/memrecd}"
DATA_DIR="${DATA_DIR:-$HOME/.memrec}"
PID_FILE="$DATA_DIR/memrecd.pid"
LOG_FILE="$DATA_DIR/memrecd.log"
SOCKET_PATH="$DATA_DIR/memrecd.sock"

mkdir -p "$DATA_DIR"

if [ -f "$PID_FILE" ]; then
    PID=$(cat "$PID_FILE")
    if ps -p "$PID" > /dev/null 2>&1; then
        echo -e "${YELLOW}memrecd is already running (PID: $PID)${NC}"
        echo "Socket: $SOCKET_PATH"
        exit 0
    else
        echo -e "${YELLOW}Stale PID file found, removing...${NC}"
        rm -f "$PID_FILE"
    fi
fi

if [ ! -x "$MEMRECD_BIN" ]; then
    echo -e "${RED}Error: memrecd binary not found at $MEMRECD_BIN${NC}"
    echo "Please build and install first:"
    echo "  cargo build --release"
    echo "  install -m 755 target/release/memrecd ~/.local/bin/"
    exit 1
fi

rm -f "$SOCKET_PATH"

echo -e "${GREEN}Starting memrecd...${NC}"
echo "Binary: $MEMRECD_BIN"
echo "Data: $DATA_DIR"
echo "Log: $LOG_FILE"

nohup "$MEMRECD_BIN" >> "$LOG_FILE" 2>&1 &
PID=$!

sleep 1

if ps -p "$PID" > /dev/null 2>&1; then
    echo $PID > "$PID_FILE"
    echo -e "${GREEN}memrecd started successfully (PID: $PID)${NC}"
    echo "Socket: $SOCKET_PATH"
    echo ""
    echo "View logs: tail -f $LOG_FILE"
    echo "Stop: $SCRIPT_DIR/stop.sh"
else
    echo -e "${RED}Failed to start memrecd${NC}"
    echo "Check logs: $LOG_FILE"
    exit 1
fi