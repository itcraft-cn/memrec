#!/bin/sh
# MemRec deb postrm / rpm %postun 脚本。
#
# 仅提示用户数据保留与用户服务的手工清理方式；不删除任何用户数据。
set -e

cat <<'EOF'

MemRec package removed. User data was preserved under ~/.memrec/.

To fully clean up per user account:
  - disable the user service:
      systemctl --user disable --now memrecd 2>/dev/null || true
  - remove data and models:
      rm -rf ~/.memrec

EOF

exit 0
