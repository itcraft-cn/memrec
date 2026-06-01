#!/bin/bash
set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$HOME/.local/bin"
DATA_DIR="$HOME/.memrec"
MODEL_DIR="$HOME/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx"
SERVICE_NAME="memrecd"
SERVICE_FILE="$HOME/.config/systemd/user/${SERVICE_NAME}.service"

MODEL_BASE_URL="https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main"
MODEL_FILES=("model.onnx" "tokenizer.json" "config.json" "special_tokens_map.json" "tokenizer_config.json")

step() {
    echo -e "\n${CYAN}>>> $1${NC}\n"
}

ok() {
    echo -e "${GREEN}[OK]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

fail() {
    echo -e "${RED}[FAIL]${NC} $1"
}

# ============================================================
# Step 0: Pre-check
# ============================================================
step "Step 0/5: Pre-check"

if ! command -v cargo &>/dev/null; then
    fail "cargo not found. Please install Rust first: https://rustup.rs"
    exit 1
fi
ok "cargo found"

if ! command -v systemctl &>/dev/null; then
    fail "systemctl not found. This script requires systemd."
    exit 1
fi
ok "systemctl found"

# ============================================================
# Step 1: Build and install binaries
# ============================================================
step "Step 1/5: Build and install binaries to ${BIN_DIR}"

if [ ! -f "${SCRIPT_DIR}/target/release/memrec" ] || [ ! -f "${SCRIPT_DIR}/target/release/memrecd" ]; then
    echo "Building release binaries..."
    cargo build --release --manifest-path "${SCRIPT_DIR}/Cargo.toml"
else
    ok "Release binaries already built"
fi

mkdir -p "${BIN_DIR}"

install -m 755 "${SCRIPT_DIR}/target/release/memrecd" "${BIN_DIR}/memrecd"
install -m 755 "${SCRIPT_DIR}/target/release/memrec" "${BIN_DIR}/memrec"

ok "memrecd -> ${BIN_DIR}/memrecd"
ok "memrec  -> ${BIN_DIR}/memrec"

# ============================================================
# Step 2: Download embedding model
# ============================================================
step "Step 2/5: Download embedding model (~90MB)"

mkdir -p "${MODEL_DIR}"

MODEL_OK=true
for f in "${MODEL_FILES[@]}"; do
    if [ ! -f "${MODEL_DIR}/${f}" ]; then
        MODEL_OK=false
        break
    fi
done

if [ "$MODEL_OK" = true ]; then
    ok "Model files already exist in ${MODEL_DIR}"
else
    DOWNLOAD_OK=true
    for f in "${MODEL_FILES[@]}"; do
        if [ -f "${MODEL_DIR}/${f}" ]; then
            ok "Already exists: ${f}"
            continue
        fi

        echo "  Downloading ${f}..."
        if command -v wget &>/dev/null; then
            if ! wget -q --show-progress -O "${MODEL_DIR}/${f}" "${MODEL_BASE_URL}/${f}"; then
                rm -f "${MODEL_DIR}/${f}"
                DOWNLOAD_OK=false
                warn "Failed to download: ${f}"
                break
            fi
        elif command -v curl &>/dev/null; then
            if ! curl -fSL -o "${MODEL_DIR}/${f}" "${MODEL_BASE_URL}/${f}"; then
                rm -f "${MODEL_DIR}/${f}"
                DOWNLOAD_OK=false
                warn "Failed to download: ${f}"
                break
            fi
        else
            DOWNLOAD_OK=false
            warn "Neither wget nor curl found"
            break
        fi
        ok "Downloaded: ${f}"
    done

    if [ "$DOWNLOAD_OK" = false ]; then
        echo ""
        warn "Model download failed. You can manually download later:"
        echo ""
        echo "  mkdir -p ${MODEL_DIR}"
        echo "  cd ${MODEL_DIR}"
        for f in "${MODEL_FILES[@]}"; do
            echo "  wget ${MODEL_BASE_URL}/${f}"
        done
        echo ""
        echo "Or set a custom model path:"
        echo "  export MEMREC_MODEL_DIR=/path/to/your/model"
        echo "  Then re-run this script: ${SCRIPT_DIR}/$0"
        echo ""
        read -p "Continue without model? (y/N) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    fi
fi

# ============================================================
# Step 3: Register systemd service
# ============================================================
step "Step 3/5: Register systemd user service"

mkdir -p "${DATA_DIR}/data"
mkdir -p "${DATA_DIR}/vectors"
mkdir -p "$(dirname "${SERVICE_FILE}")"

cat > "${SERVICE_FILE}" << EOF
[Unit]
Description=MemRec Memory Persistence Daemon
Documentation=https://github.com/anomalyco/memrec
After=default.target

[Service]
Type=simple
ExecStart=${BIN_DIR}/memrecd
ExecStopPost=/bin/rm -f ${DATA_DIR}/memrecd.sock
Restart=on-failure
RestartSec=5

Environment="RUST_LOG=info"

WorkingDirectory=${DATA_DIR}

StandardOutput=append:${DATA_DIR}/memrecd.log
StandardError=append:${DATA_DIR}/memrecd.log

[Install]
WantedBy=default.target
EOF

ok "Service file: ${SERVICE_FILE}"

# Stop existing service if running
if systemctl --user is-active --quiet "${SERVICE_NAME}" 2>/dev/null; then
    echo "Stopping existing service..."
    systemctl --user stop "${SERVICE_NAME}"
fi

systemctl --user daemon-reload
systemctl --user enable "${SERVICE_NAME}"
systemctl --user start "${SERVICE_NAME}"

sleep 2

if systemctl --user is-active --quiet "${SERVICE_NAME}"; then
    ok "Service ${SERVICE_NAME} is running (PID: $(systemctl --user show -p MainPID --value ${SERVICE_NAME}))"
else
    fail "Service failed to start. Check logs:"
    echo "  systemctl --user status ${SERVICE_NAME}"
    echo "  cat ${DATA_DIR}/memrecd.log"
    exit 1
fi

# ============================================================
# Step 4: Test write and read
# ============================================================
step "Step 4/5: Test write and read"

TEST_CONTENT="MemRec installation test - $(date -Iseconds)"
TEST_ID=""

echo "Writing test memory..."
TEST_OUTPUT=$("${BIN_DIR}/memrec" add "${TEST_CONTENT}" --mtype knowledge --tag test 2>&1) || {
    fail "Write test failed: ${TEST_OUTPUT}"
    exit 1
}

TEST_ID=$(echo "${TEST_OUTPUT}" | grep -oP 'Added memory: \K[0-9a-f-]+' || true)

if [ -z "${TEST_ID}" ]; then
    # Try alternate parsing
    TEST_ID=$(echo "${TEST_OUTPUT}" | head -1 | awk '{print $NF}')
fi

if [ -n "${TEST_ID}" ]; then
    ok "Write success: ${TEST_ID}"
else
    warn "Write completed but could not parse memory ID"
fi

sleep 1

echo "Reading test memory..."
READ_OUTPUT=$("${BIN_DIR}/memrec" search "installation test" --project-only 2>&1) || {
    fail "Read test failed: ${READ_OUTPUT}"
    exit 1
}

TOTAL=$(echo "${READ_OUTPUT}" | grep -oP '"total":\s*\K[0-9]+' || echo "0")

if [ "${TOTAL}" -gt 0 ] 2>/dev/null; then
    ok "Read success: found ${TOTAL} result(s)"
else
    warn "Search returned no results (may need model). Trying list..."
    LIST_OUTPUT=$("${BIN_DIR}/memrec" list --limit 1 2>&1)
    if echo "${LIST_OUTPUT}" | grep -q "Found"; then
        ok "List works: $(echo "${LIST_OUTPUT}" | head -1)"
    else
        fail "Read test failed"
        exit 1
    fi
fi

# Clean up test memory
if [ -n "${TEST_ID}" ]; then
    "${BIN_DIR}/memrec" delete "${TEST_ID}" &>/dev/null || true
    ok "Test memory cleaned up"
fi

# Version check
VERSION_OUTPUT=$("${BIN_DIR}/memrec" version 2>&1)
ok "${VERSION_OUTPUT}"

# ============================================================
# Step 5: Summary
# ============================================================
step "Step 5/5: Installation complete"

echo -e "${GREEN}╔══════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║         MemRec installed successfully!              ║${NC}"
echo -e "${GREEN}╠══════════════════════════════════════════════════════╣${NC}"
echo -e "${GREEN}║${NC}                                                      ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}  Binaries:  ${BIN_DIR}/memrec, memrecd               ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}  Data:      ${DATA_DIR}/                             ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}  Model:     ${MODEL_DIR}/  ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}  Service:   systemd --user (${SERVICE_NAME})        ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}  Socket:    ${DATA_DIR}/memrecd.sock                ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}  Log:       ${DATA_DIR}/memrecd.log                 ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}                                                      ${GREEN}║${NC}"
echo -e "${GREEN}╠══════════════════════════════════════════════════════╣${NC}"
echo -e "${GREEN}║${NC}  Quick start:                                        ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}    memrec add \"hello\" --mtype knowledge             ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}    memrec search \"hello\"                            ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}    memrec stats                                      ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}                                                      ${GREEN}║${NC}"
echo -e "${GREEN}╠══════════════════════════════════════════════════════╣${NC}"
echo -e "${GREEN}║${NC}  Service management:                                 ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}    systemctl --user status memrecd                  ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}    systemctl --user restart memrecd                 ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}    systemctl --user stop memrecd                    ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}                                                      ${GREEN}║${NC}"
echo -e "${GREEN}╠══════════════════════════════════════════════════════╣${NC}"
echo -e "${GREEN}║${NC}  Environment variables:                              ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}    MEMREC_MODEL_DIR  - custom model path             ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}    MEMREC_MIN_SCORE  - min search score (0.75)      ${GREEN}║${NC}"
echo -e "${GREEN}║${NC}                                                      ${GREEN}║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════════════════╝${NC}"
