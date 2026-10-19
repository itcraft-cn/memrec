#!/usr/bin/env bash
# MemRec 打包入口脚本（nfpm 工具链）
#
# 职责：
#   1. 从仓库版本单一事实源（.version / workspace Cargo.toml）解析版本号与发布号；
#   2. 构建 release 二进制；
#   3. 通过 nfpm 由同一份 packaging/nfpm.yaml 产出 deb 与 rpm；
#   4. 将产物统一归档到 release/ 目录。
#
# 用法：
#   scripts/package/build-packages.sh              # 构建并打包 deb + rpm
#   scripts/package/build-packages.sh deb          # 仅打 deb
#   scripts/package/build-packages.sh rpm          # 仅打 rpm
#   SKIP_BUILD=1 scripts/package/build-packages.sh # 跳过 cargo build（复用已有产物）
#   PKG_VERSION=0.11.0 PKG_RELEASE=1 scripts/package/build-packages.sh  # 覆盖版本/发布号
#
# 环境变量：
#   PKG_VERSION   覆盖版本号（默认由版本事实源推导）
#   PKG_RELEASE   覆盖发布号（默认 dev1 / 1）
#   SKIP_BUILD    非空时跳过 cargo build --release
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

PACKAGERS=("$@")
if [ ${#PACKAGERS[@]} -eq 0 ]; then
    PACKAGERS=(deb rpm)
fi

for p in "${PACKAGERS[@]}"; do
    case "$p" in
        deb|rpm) ;;
        *)
            echo "[FAIL] unsupported packager: $p (expected: deb, rpm)" >&2
            exit 1
            ;;
    esac
done

# 解析版本事实源：优先 .version，其次 workspace Cargo.toml。
resolve_raw_version() {
    if [ -f "$ROOT_DIR/.version" ]; then
        tr -d '[:space:]' < "$ROOT_DIR/.version"
        return
    fi
    sed -n '/^\[workspace\.package\]/,/^\[/p' "$ROOT_DIR/Cargo.toml" \
        | sed -n 's/^version *= *"\(.*\)".*/\1/p' | head -n1
}

RAW_VERSION="$(resolve_raw_version)"
if [ -z "$RAW_VERSION" ]; then
    echo "[FAIL] cannot resolve version from .version or Cargo.toml" >&2
    exit 1
fi

# -dev 后缀拆分为 VERSION + RELEASE：0.11.0-dev -> 0.11.0 / dev1
if [[ "$RAW_VERSION" =~ ^([0-9]+\.[0-9]+\.[0-9]+)-dev$ ]]; then
    VERSION="${PKG_VERSION:-${BASH_REMATCH[1]}}"
    RELEASE="${PKG_RELEASE:-dev1}"
else
    VERSION="${PKG_VERSION:-$RAW_VERSION}"
    RELEASE="${PKG_RELEASE:-1}"
fi

# 架构映射：uname -m -> nfpm 约定架构名
case "$(uname -m)" in
    x86_64)  ARCH="amd64" ;;
    aarch64) ARCH="arm64" ;;
    *)
        echo "[FAIL] unsupported architecture: $(uname -m)" >&2
        exit 1
        ;;
esac

echo ">>> MemRec packaging"
echo "    version : $VERSION"
echo "    release : $RELEASE"
echo "    arch    : $ARCH"
echo "    formats : ${PACKAGERS[*]}"

if [ -z "${SKIP_BUILD:-}" ]; then
    echo ">>> Building release binaries ..."
    cargo build --release
fi

for bin in memrec memrecd mr-install; do
    if [ ! -x "$ROOT_DIR/target/release/$bin" ]; then
        echo "[FAIL] missing binary: target/release/$bin (run without SKIP_BUILD)" >&2
        exit 1
    fi
done

echo ">>> Locating nfpm ..."
NFPM="$(scripts/package/ensure-nfpm.sh)"

mkdir -p "$ROOT_DIR/release"

export VERSION RELEASE ARCH
for p in "${PACKAGERS[@]}"; do
    echo ">>> Packaging $p ..."
    "$NFPM" package -f "$ROOT_DIR/packaging/nfpm.yaml" -p "$p" -t "$ROOT_DIR/release"
done

echo ""
echo ">>> Artifacts in release/:"
ls -lh "$ROOT_DIR/release" | grep -E '\.(deb|rpm)$' || true
echo ""
echo "Done. Verify with: dpkg-deb -I release/*.deb  /  rpm -qip release/*.rpm"
