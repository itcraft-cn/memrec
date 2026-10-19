#!/bin/sh
# MemRec deb postinst / rpm %post 脚本。
#
# 设计取向：不在安装时自动下载模型。
# 模型约数 GB，下载耗时且依赖网络，交由用户在交互环境中执行；
# 本脚本仅打印后续操作指引，不修改任何用户数据目录。
set -e

cat <<'EOF'

MemRec has been installed. Next steps (run once per user account):

  1) Download the embedding model and generate the config:
       mr-install --skip-install --skip-cargo-check --skip-service

     - --skip-install      : binaries are already provided by this package
     - --skip-cargo-check  : no Rust toolchain required
     - --skip-service      : use the systemd user unit shipped by this package

     Optional: add --use-hf-mirror for faster downloads in mainland China.

  2) Enable and start the daemon (systemd user service):
       systemctl --user daemon-reload
       systemctl --user enable --now memrecd

  3) Verify:
       memrec stats

Notes:
  - Data and models live in ~/.memrec/ (per user), never removed by the package.
  - Do NOT run `mr-install` without --skip-install: it would install a second
    copy under ~/.local/bin and shadow the packaged binaries.

EOF

exit 0
