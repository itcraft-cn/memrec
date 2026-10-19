#!/usr/bin/env bash
# 定位 nfpm：优先使用系统已安装版本，缺失时下载官方预编译二进制到 scripts/tools/bin/。
#
# 输出：stdout 打印可用的 nfpm 可执行文件绝对路径，供 build-packages.sh 捕获。
#
# 环境变量：
#   NFPM_VERSION  下载使用的 nfpm 版本（默认 2.48.0）
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
NFPM_VERSION="${NFPM_VERSION:-2.48.0}"
TOOLS_BIN="$ROOT_DIR/scripts/tools/bin"
LOCAL_NFPM="$TOOLS_BIN/nfpm"

# 1) 系统已安装则直接使用
if command -v nfpm >/dev/null 2>&1; then
    command -v nfpm
    exit 0
fi

# 2) 本地缓存命中则复用
if [ -x "$LOCAL_NFPM" ]; then
    echo "$LOCAL_NFPM"
    exit 0
fi

# 3) 下载官方预编译二进制
case "$(uname -m)" in
    x86_64)  ARCH_TAG="x86_64" ;;
    aarch64) ARCH_TAG="arm64" ;;
    *) echo "[FAIL] unsupported architecture for nfpm download: $(uname -m)" >&2; exit 1 ;;
esac

URL="https://github.com/goreleaser/nfpm/releases/download/v${NFPM_VERSION}/nfpm_${NFPM_VERSION}_Linux_${ARCH_TAG}.tar.gz"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "[nfpm] downloading $URL ..." >&2
curl -fsSL "$URL" -o "$TMP_DIR/nfpm.tar.gz"
tar -xzf "$TMP_DIR/nfpm.tar.gz" -C "$TMP_DIR" nfpm
mkdir -p "$TOOLS_BIN"
install -m 0755 "$TMP_DIR/nfpm" "$LOCAL_NFPM"

echo "$LOCAL_NFPM"
