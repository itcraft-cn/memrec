#!/bin/bash
# Stop memrecd daemon manually (without systemd)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

DATA_DIR="${DATA_DIR:-$HOME/.memrec}"
PID_FILE="$DATA_DIR/memrecd.pid"
SOCKET_PATH="$DATA_DIR/memrecd.sock"

if [ ! -f "$PID_FILE" ]; then
    echo -e "${YELLOW}memrecd is not running (no PID file)${NC}"
    rm -f "$SOCKET_PATH"
    exit 0
fi

PID=$(cat "$PID_FILE")

if ! ps -p "$PID" > /dev/null 2>&1; then
    echo -e "${YELLOW}memrecd is not running (stale PID file)${NC}"
    rm -f "$PID_FILE"
    rm -f "$SOCKET_PATH"
    exit 0
fi

echo -e "${YELLOW}Stopping memrecd (PID: $PID)...${NC}"

kill -TERM "$PID" 2>/dev/null || true

TIMEOUT=10
for i in $(seq 1 $TIMEOUT); do
    if ! ps -p "$PID" > /dev/null 2>&1; then
        break
    fi
    sleep 1
done

if ps -p "$PID" > /dev/null 2>&1; then
    echo -e "${YELLOW}Process didn't stop gracefully, forcing...${NC}"
    kill -9 "$PID" 2>/dev/null || true
    sleep 1
fi

if ps -p "$PID" > /dev/null 2>&1; then
    echo -e "${RED}Failed to stop memrecd (PID: $PID)${NC}"
    exit 1
fi

rm -f "$PID_FILE"
rm -f "$SOCKET_PATH"

echo -e "${GREEN}memrecd stopped successfully${NC}"