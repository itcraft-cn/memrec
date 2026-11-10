#!/usr/bin/env bash
#
# 测试期禁写开关
#
# 用法：
#   scripts/test-mode.sh start    # 创建 ~/.memrec/.debug，生产 socket 立即禁写
#   scripts/test-mode.sh stop     # 删除开关文件，恢复生产 socket 写入
#   scripts/test-mode.sh status   # 查看当前状态
#
# 说明：
#   - 开关文件存在期间，memrecd 生产 socket 拒绝变更类请求（-32010），
#     读请求不受影响；
#   - 测试端必须使用测试 socket：~/.memrec/memrecd-test.sock
#     （CLI/MCP 通过 MEMREC_SOCKET_PATH 指定；DSH 插件通过 socketPath 配置）；
#   - 开关文件超过 test_mode_max_age_secs（默认 6 小时）视为残留并自动忽略，
#     仍建议 stop 或让脚本的 trap 负责清理；
#   - Dream 在测试期间会跳过执行。
#
# 典型用法：
#   scripts/test-mode.sh start
#   export MEMREC_SOCKET_PATH="$HOME/.memrec/memrecd-test.sock"
#   ... 跑测试 ...
#   scripts/test-mode.sh stop
set -euo pipefail

MEMREC_DIR="${MEMREC_HOME:-$HOME/.memrec}"
FLAG="$MEMREC_DIR/.debug"
TEST_SOCK="${MEMREC_TEST_SOCKET:-$MEMREC_DIR/memrecd-test.sock}"

cmd="${1:-status}"

case "$cmd" in
  start)
    if [ -e "$FLAG" ]; then
      echo "test mode already active: $FLAG" >&2
      exit 1
    fi
    umask 077
    printf '{"started_at":"%s","pid":%s}\n' "$(date -Is)" "$$" > "$FLAG"
    echo "test mode ON  -> production socket rejects writes"
    echo "export MEMREC_SOCKET_PATH=$TEST_SOCK"
    ;;
  stop)
    rm -f "$FLAG"
    echo "test mode OFF"
    ;;
  status)
    if [ -e "$FLAG" ]; then
      echo "test mode ON ($FLAG)"
      echo "test socket: $TEST_SOCK"
    else
      echo "test mode OFF"
    fi
    ;;
  *)
    echo "usage: $0 {start|stop|status}" >&2
    exit 2
    ;;
esac
