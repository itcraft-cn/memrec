#!/usr/bin/env bash
#
# 多步骤测试与部署（严格按序）
#
# 对应 AGENTS.md / MEMORY.md 中的验收流程：
#   1. cargo build --release
#   2. cargo test --release --workspace
#   3. systemctl --user stop memrecd
#   4. 备份 ~/.memrec（数据）与当前已安装二进制
#   5. 部署 memrec / memrecd 到 ~/.local/bin 与 ~/.cargo/bin
#   6. systemctl --user start memrecd 并等待 socket 就绪
#   7. scripts/retrieval_gate.sh（可跳过）
#
# 任一前置步骤失败都不会停止生产服务；只有构建与测试通过后才进入停服阶段。
# 门禁失败不会自动回滚数据，脚本会打印备份路径与恢复命令，由人决定下一步。
#
# 用法：
#   scripts/multistep-test.sh run [--skip-tests] [--skip-gate]
#   scripts/multistep-test.sh restore <data-backup-dir> [--yes]
#   scripts/multistep-test.sh restore-bin [--yes]
#   scripts/multistep-test.sh list
#   scripts/multistep-test.sh prune --keep N [--yes]
#   scripts/multistep-test.sh status
#
# 环境变量：
#   MEMREC_HOME      数据目录，默认 ~/.memrec
#   MEMREC_SERVICE   systemd 用户服务名，默认 memrecd
#   MEMREC_BIN_DIRS  部署目录（空格分隔），默认 "~/.local/bin ~/.cargo/bin"
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MEMREC_HOME="${MEMREC_HOME:-$HOME/.memrec}"
SERVICE="${MEMREC_SERVICE:-memrecd}"
BIN_DIRS_STR="${MEMREC_BIN_DIRS:-$HOME/.local/bin $HOME/.cargo/bin}"
# shellcheck disable=SC2206
BIN_DIRS=($BIN_DIRS_STR)
BINS=(memrec memrecd)
SOCKET="$MEMREC_HOME/memrecd.sock"
TEST_FLAG="$MEMREC_HOME/.debug"
BIN_BACKUP="$MEMREC_HOME/.bin-backup"
TS="$(date +%Y%m%d_%H%M%S)"
DATA_BACKUP="$HOME/.memrec.bak.$TS"
LOG_FILE="${MEMREC_DEPLOY_LOG:-/tmp/.memrec.deploy.log}"

# 日志落在 /tmp，重启即丢弃；首次创建用 0600，并拒绝符号链接。
init_log() {
  [ -n "$LOG_FILE" ] || return 0
  if [ -L "$LOG_FILE" ]; then
    printf 'WARN: 日志路径是符号链接，已禁用文件日志: %s\n' "$LOG_FILE" >&2
    LOG_FILE=""
    return 0
  fi
  if [ ! -e "$LOG_FILE" ]; then
    (umask 077; : >>"$LOG_FILE") 2>/dev/null || true
  fi
  chmod 600 "$LOG_FILE" 2>/dev/null || true
  if [ ! -w "$LOG_FILE" ]; then
    printf 'WARN: 日志不可写，已禁用文件日志: %s\n' "$LOG_FILE" >&2
    LOG_FILE=""
  fi
}

log() {
  local line
  line="[$(date '+%Y-%m-%d %H:%M:%S')] $*"
  printf '%s\n' "$line"
  if [ -n "$LOG_FILE" ]; then
    printf '%s\n' "$line" >>"$LOG_FILE" 2>/dev/null || LOG_FILE=""
  fi
}

die() {
  log "ERROR: $*" >&2
  exit 1
}

require() {
  command -v "$1" >/dev/null 2>&1 || die "缺少命令: $1"
}

service_active() {
  systemctl --user is-active --quiet "$SERVICE"
}

wait_for_service() {
  local i
  for i in $(seq 1 60); do
    if service_active && [ -S "$SOCKET" ]; then
      return 0
    fi
    sleep 1
  done
  return 1
}

check_env() {
  [ "$(id -u)" -ne 0 ] || die "请以普通用户运行（systemctl --user）"
  require systemctl
  require install
  command -v cargo >/dev/null 2>&1 || die "缺少 cargo"
}

backup_binaries() {
  if [ ! -d "$MEMREC_HOME" ]; then
    log "数据目录不存在，跳过二进制备份: $MEMREC_HOME"
    return 0
  fi
  mkdir -p "$BIN_BACKUP"
  local found=0 dir bin
  for dir in "${BIN_DIRS[@]}"; do
    for bin in "${BINS[@]}"; do
      if [ -f "$dir/$bin" ]; then
        install -m 755 "$dir/$bin" "$BIN_BACKUP/$bin"
        found=1
      fi
    done
  done
  if [ "$found" -eq 1 ]; then
    log "已备份当前二进制 -> $BIN_BACKUP"
  else
    log "未找到已安装二进制，跳过备份"
  fi
}

check_disk_space() {
  local need avail
  need="$(du -sm "$MEMREC_HOME" 2>/dev/null | cut -f1 || echo 0)"
  avail="$(df -Pm "$(dirname "$MEMREC_HOME")" | awk 'NR==2 {print $4}')"
  if [ "${need:-0}" -gt 0 ] && [ "${avail:-0}" -lt "$need" ]; then
    die "磁盘空间不足：需要 ${need}MB，可用 ${avail}MB"
  fi
}

cmd_run() {
  local skip_tests=0 skip_gate=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --skip-tests) skip_tests=1 ;;
      --skip-gate) skip_gate=1 ;;
      *) die "未知参数: $1" ;;
    esac
    shift
  done

  check_env

  log "1/7 编译: cargo build --release"
  (cd "$ROOT" && cargo build --release)

  if [ "$skip_tests" -eq 1 ]; then
    log "2/7 单测: 跳过（--skip-tests）"
  else
    log "2/7 单测: cargo test --release --workspace"
    (cd "$ROOT" && cargo test --release --workspace)
  fi

  log "3/7 停止服务: $SERVICE"
  if service_active; then
    systemctl --user stop "$SERVICE"
    log "服务已停止"
  else
    log "服务未运行，跳过停止"
  fi

  log "4/7 备份数据 -> $DATA_BACKUP"
  [ -d "$MEMREC_HOME" ] || die "数据目录不存在: $MEMREC_HOME"
  check_disk_space
  cp -a "$MEMREC_HOME" "$DATA_BACKUP"
  backup_binaries

  log "5/7 部署二进制到 ${BIN_DIRS[*]}"
  local bin dir
  for bin in "${BINS[@]}"; do
    [ -x "$ROOT/target/release/$bin" ] || die "缺少构建产物: target/release/$bin"
  done
  for dir in "${BIN_DIRS[@]}"; do
    mkdir -p "$dir"
    for bin in "${BINS[@]}"; do
      install -m 755 "$ROOT/target/release/$bin" "$dir/$bin"
    done
  done

  log "6/7 启动服务: $SERVICE"
  systemctl --user start "$SERVICE"
  if ! wait_for_service; then
    log "服务未在 60 秒内就绪，最近日志："
    journalctl --user -u "$SERVICE" -n 30 --no-pager || true
    die "服务启动失败。数据备份: $DATA_BACKUP；二进制备份: $BIN_BACKUP"
  fi
  log "服务已就绪: $(systemctl --user show -p ActiveState --value "$SERVICE")"

  if [ "$skip_gate" -eq 1 ]; then
    log "7/7 检索门禁: 跳过（--skip-gate）"
  else
    log "7/7 检索门禁: scripts/retrieval_gate.sh"
    if ! (cd "$ROOT" && MEMREC_BIN="${MEMREC_BIN:-$(command -v memrec)}" bash scripts/retrieval_gate.sh); then
      log "检索门禁失败：服务保持运行，未自动回滚数据"
      log "  数据备份: $DATA_BACKUP"
      log "  二进制备份: $BIN_BACKUP"
      log "  回滚数据: $0 restore \"$DATA_BACKUP\""
      log "  回滚二进制: $0 restore-bin --yes"
      exit 1
    fi
  fi

  if [ -e "$TEST_FLAG" ]; then
    log "注意: 测试开关仍存在（$TEST_FLAG），生产 socket 写入处于禁用状态"
  fi

  log "完成：构建/测试/备份/部署/门禁 全部通过"
  log "  数据备份: $DATA_BACKUP"
  log "  二进制备份: $BIN_BACKUP"
  log "  服务状态: $(systemctl --user show -p ActiveState --value "$SERVICE")"
  log "  当前记忆数: $(memrec stats 2>/dev/null | sed -n 's/.*"active_memories": *\([0-9]*\).*/\1/p' | tr -d '\n')"
}

cmd_restore() {
  local backup="" yes=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --yes|-y) yes=1 ;;
      *) [ -z "$backup" ] || die "多余参数: $1"; backup="$1" ;;
    esac
    shift
  done
  [ -n "$backup" ] || die "用法: $0 restore <data-backup-dir> [--yes]"
  [ -d "$backup" ] || die "备份目录不存在: $backup"
  check_env

  if [ "$yes" -ne 1 ]; then
    printf '将停服并把当前数据移走，然后用 %s 覆盖 %s。继续？[y/N] ' "$backup" "$MEMREC_HOME"
    read -r reply
    case "$reply" in y|Y|yes|YES) ;; *) die "已取消" ;; esac
  fi

  log "停止服务: $SERVICE"
  if service_active; then systemctl --user stop "$SERVICE"; fi

  if [ -e "$MEMREC_HOME" ]; then
    local aside="$HOME/.memrec.before-restore.$TS"
    log "移走当前数据 -> $aside"
    mv "$MEMREC_HOME" "$aside"
  fi

  log "恢复数据: $backup -> $MEMREC_HOME"
  cp -a "$backup" "$MEMREC_HOME"

  log "启动服务: $SERVICE"
  systemctl --user start "$SERVICE"
  wait_for_service || die "恢复后服务未就绪，请检查 journalctl --user -u $SERVICE"
  log "恢复完成: $(systemctl --user show -p ActiveState --value "$SERVICE")"
}

cmd_restore_bin() {
  local yes=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --yes|-y) yes=1 ;;
      *) die "未知参数: $1" ;;
    esac
    shift
  done
  [ -d "$BIN_BACKUP" ] || die "没有二进制备份: $BIN_BACKUP"
  check_env
  if [ "$yes" -ne 1 ]; then
    printf '将用 %s 覆盖已安装的 memrec/memrecd。继续？[y/N] ' "$BIN_BACKUP"
    read -r reply
    case "$reply" in y|Y|yes|YES) ;; *) die "已取消" ;; esac
  fi
  local dir bin
  for dir in "${BIN_DIRS[@]}"; do
    for bin in "${BINS[@]}"; do
      [ -f "$BIN_BACKUP/$bin" ] || continue
      install -m 755 "$BIN_BACKUP/$bin" "$dir/$bin"
    done
  done
  log "二进制已恢复自 $BIN_BACKUP（如服务运行中，建议 restart）"
}

cmd_list() {
  log "数据备份（新 -> 旧）："
  # shellcheck disable=SC2012
  ls -1dt "$HOME"/.memrec.bak.* 2>/dev/null | while read -r d; do
    [ -d "$d" ] && printf '  %s  %s\n' "$(du -sh "$d" 2>/dev/null | cut -f1)" "$d"
  done || true
  if [ -d "$BIN_BACKUP" ]; then
    log "二进制备份: $BIN_BACKUP"
    ls -l "$BIN_BACKUP" 2>/dev/null || true
  fi
}

cmd_prune() {
  local keep=10 yes=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --keep) shift; keep="${1:-}"; [ -n "$keep" ] || die "--keep 需要参数" ;;
      --yes|-y) yes=1 ;;
      *) die "未知参数: $1" ;;
    esac
    shift
  done
  case "$keep" in ''|*[!0-9]*) die "--keep 必须是非负整数" ;; esac
  local backups=()
  while IFS= read -r d; do backups+=("$d"); done < <(ls -1dt "$HOME"/.memrec.bak.* 2>/dev/null || true)
  local total=${#backups[@]}
  if [ "$total" -le "$keep" ]; then
    log "备份数 $total <= $keep，无需清理"
    return 0
  fi
  log "将删除 $((total - keep)) 个旧备份（保留最近 $keep 个）："
  local i
  for ((i = keep; i < total; i++)); do log "  ${backups[$i]}"; done
  if [ "$yes" -ne 1 ]; then
    printf '继续？[y/N] '
    read -r reply
    case "$reply" in y|Y|yes|YES) ;; *) die "已取消" ;; esac
  fi
  for ((i = keep; i < total; i++)); do rm -rf "${backups[$i]}"; done
  log "清理完成"
}

cmd_status() {
  check_env
  log "服务: $SERVICE -> $(systemctl --user show -p ActiveState --value "$SERVICE" 2>/dev/null || echo unknown)"
  if [ -S "$SOCKET" ]; then
    log "生产 socket: $SOCKET"
  else
    log "生产 socket: 不存在"
  fi
  if [ -S "$MEMREC_HOME/memrecd-test.sock" ]; then
    log "测试 socket: $MEMREC_HOME/memrecd-test.sock"
  fi
  if [ -e "$TEST_FLAG" ]; then
    log "测试模式: ON（生产 socket 禁写）"
  else
    log "测试模式: OFF"
  fi
  local bin dir
  for bin in "${BINS[@]}"; do
    for dir in "${BIN_DIRS[@]}"; do
      if [ -f "$dir/$bin" ]; then
        log "$bin: $dir/$bin ($(stat -c '%y' "$dir/$bin" | cut -d. -f1))"
        break
      fi
    done
  done
  if service_active; then
    memrec stats 2>/dev/null | sed -n 's/.*"active_memories": *\([0-9]*\).*/活跃记忆: \1/p' | tr -d '\n' || true
    echo
  fi
}

usage() {
  awk 'NR>1 { if ($0 ~ /^set -euo/) exit; sub(/^# ?/, ""); print }' "$0"
}

main() {
  init_log
  local cmd="${1:-}"
  case "$cmd" in
    run) shift; cmd_run "$@" ;;
    restore) shift; cmd_restore "$@" ;;
    restore-bin) shift; cmd_restore_bin "$@" ;;
    list) shift; cmd_list "$@" ;;
    prune) shift; cmd_prune "$@" ;;
    status) shift; cmd_status "$@" ;;
    ""|-h|--help|help) usage ;;
    *) die "未知子命令: $cmd（可用: run|restore|restore-bin|list|prune|status）" ;;
  esac
}

main "$@"
