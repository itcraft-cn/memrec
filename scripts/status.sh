#!/bin/bash
# Show memrecd daemon status (without systemd)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

DATA_DIR="${DATA_DIR:-$HOME/.memrec}"
PID_FILE="$DATA_DIR/memrecd.pid"
SOCKET_PATH="$DATA_DIR/memrecd.sock"
LOG_FILE="$DATA_DIR/memrecd.log"
DB_PATH="$DATA_DIR/db"

echo -e "${BLUE}=== memrecd Status ===${NC}"
echo ""

if [ -f "$PID_FILE" ]; then
    PID=$(cat "$PID_FILE")
    if ps -p "$PID" > /dev/null 2>&1; then
        echo -e "Status: ${GREEN}Running${NC}"
        echo "PID: $PID"
        
        START_TIME=$(ps -p "$PID" -o lstart= 2>/dev/null | xargs)
        if [ -n "$START_TIME" ]; then
            echo "Started: $START_TIME"
        fi
        
        CPU_MEM=$(ps -p "$PID" -o %cpu,%mem --no-headers 2>/dev/null)
        if [ -n "$CPU_MEM" ]; then
            echo "CPU/MEM: $CPU_MEM"
        fi
    else
        echo -e "Status: ${RED}Not Running${NC} (stale PID file)"
    fi
else
    echo -e "Status: ${RED}Not Running${NC}"
fi

echo ""
echo -e "${BLUE}--- Paths ---${NC}"
echo "Data directory: $DATA_DIR"
echo "Socket: $SOCKET_PATH"
[ -f "$LOG_FILE" ] && echo "Log file: $LOG_FILE"
[ -d "$DB_PATH" ] && echo "Database: $DB_PATH"
echo ""

if [ -S "$SOCKET_PATH" ]; then
    echo -e "${BLUE}--- Socket ---${NC}"
    ls -lh "$SOCKET_PATH" 2>/dev/null || echo "Socket exists but cannot access"
    echo ""
fi

if [ -d "$DB_PATH" ]; then
    echo -e "${BLUE}--- Database ---${NC}"
    DB_SIZE=$(du -sh "$DB_PATH" 2>/dev/null | cut -f1)
    echo "Size: $DB_SIZE"
    echo ""
fi

if [ -f "$LOG_FILE" ]; then
    echo -e "${BLUE}--- Recent Logs (last 10 lines) ---${NC}"
    tail -10 "$LOG_FILE"
    echo ""
fi

if command -v memrec &> /dev/null; then
    echo -e "${BLUE}--- Memory Stats ---${NC}"
    memrec stats 2>/dev/null || echo "Unable to get stats (daemon may not be running)"
fi