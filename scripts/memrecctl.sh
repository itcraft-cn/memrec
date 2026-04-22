#!/bin/bash
# Convenience script for memrecd service management

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

usage() {
    echo "Usage: $0 {start|stop|restart|status|logs|install|uninstall}"
    echo ""
    echo "Commands:"
    echo "  start     - Start memrecd daemon"
    echo "  stop      - Stop memrecd daemon"
    echo "  restart   - Restart memrecd daemon"
    echo "  status    - Show detailed status"
    echo "  logs      - View recent logs"
    echo "  install   - Install systemd service"
    echo "  uninstall - Remove systemd service"
    exit 1
}

if [ $# -eq 0 ]; then
    usage
fi

case "$1" in
    start)
        echo -e "${GREEN}Starting memrecd...${NC}"
        systemctl --user start memrecd.service
        sleep 1
        systemctl --user status memrecd.service --no-pager || true
        ;;
    stop)
        echo -e "${YELLOW}Stopping memrecd...${NC}"
        systemctl --user stop memrecd.service
        echo -e "${GREEN}Stopped${NC}"
        ;;
    restart)
        echo -e "${YELLOW}Restarting memrecd...${NC}"
        systemctl --user restart memrecd.service
        sleep 1
        systemctl --user status memrecd.service --no-pager || true
        ;;
    status)
        scripts/systemd/status.sh
        ;;
    logs)
        LOG_FILE="$HOME/.memrec/memrecd.log"
        if [ -f "$LOG_FILE" ]; then
            echo "Showing logs (Ctrl+C to exit):"
            tail -f "$LOG_FILE"
        else
            echo -e "${RED}Log file not found: $LOG_FILE${NC}"
            echo "Alternative: journalctl --user -u memrecd -f"
        fi
        ;;
    install)
        scripts/systemd/install.sh
        ;;
    uninstall)
        scripts/systemd/uninstall.sh
        ;;
    *)
        usage
        ;;
esac