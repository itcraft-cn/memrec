# MemRec 发布流程手册

本文档面向发布者，描述从版本号更新到发行包产出与发布的完整流程。
打包工具细节见 [packaging/README.md](../packaging/README.md)。

## 目录

1. 前置条件
2. 发布步骤概览
3. 步骤详解
4. 版本管理规则
5. 发行包与校验
6. 发布与归档
7. 常见问题

## 1. 前置条件

| 工具 | 用途 |
|------|------|
| Rust / Cargo | 构建 release 二进制 |
| nfpm（>= 2.48）| 生成 deb/rpm；缺失时 `scripts/package/ensure-nfpm.sh` 会自动下载 |
| dpkg-deb | 校验 deb |
| rpm（可选）| 校验 rpm |
| git | 提交与打标签（本项目仅本地提交，不推送） |

版本事实源：仓库根 `.version`（由 `updgitver` 维护），其次 `Cargo.toml` 的
`[workspace.package].version`。

## 2. 发布步骤概览

```
① 更新版本号（updgitver）
       ↓
② 同步 crate 间依赖版本（易漏，见第 4 节）
       ↓
③ 更新 CHANGELOG
       ↓
④ cargo build --release && cargo test --release
       ↓
⑤ scripts/package/build-packages.sh 产出 deb/rpm 到 release/
       ↓
⑥ 校验产物
       ↓
⑦ 打标签并本地提交
```

## 3. 步骤详解

### 3.1 更新版本号

使用 `updgitver` 更新 `.version` 与各 crate 的 `version` 字段。

### 3.2 同步 crate 间依赖版本

`updgitver` 只更新各 crate 的 `package.version`，不会同步 crate 之间
`[dependencies]` 中带版本要求的路径依赖。若不同步，会因版本要求与本地 crate
版本不匹配导致整个 workspace 无法编译（报 `failed to select a version`）。

需要检查并同步的文件：

- `mr-protocol/Cargo.toml` -> `mr-common`
- `mr-ability/Cargo.toml` -> `mr-common`、`mr-protocol`
- `memrec/Cargo.toml` -> `mr-common`、`mr-protocol`
- `memrecd/Cargo.toml` -> `mr-common`、`mr-ability`、`mr-protocol`
- `mr-install/Cargo.toml` -> `mr-common`

规则：这些路径依赖的版本要求应等于当前 workspace 的 dev 版本
（如 `0.11.0-dev`），与包自身 `version` 保持一致。

### 3.3 更新 CHANGELOG

在 `CHANGELOG.md` 与 `CHANGELOG_en.md` 追加本次版本条目，并同步版本标题。
（当前仓库以 `docs: bump CHANGELOG version ...` 提交维护。）

### 3.4 构建与测试

```bash
cargo build --release
cargo test --release
```

如上一步版本同步遗漏，构建会在此直接失败。

### 3.5 产出 deb/rpm

```bash
scripts/package/build-packages.sh        # deb + rpm
scripts/package/build-packages.sh deb    # 仅 deb
```

脚本会：解析版本 -> `cargo build --release` -> 调用 nfpm -> 归档到 `release/`。

### 3.6 校验产物

```bash
dpkg-deb -I release/*.deb
dpkg-deb -c release/*.deb
rpm -qip release/*.rpm    # 需安装 rpm 工具
```

确认：版本、依赖列表、文件清单（`/usr/bin/*`、`/usr/lib/systemd/user/memrecd.service`、
文档）、维护者脚本（postinst/postrm）均符合预期。

### 3.7 打标签与提交

```bash
git add -A
git commit -m "release: vX.Y.Z"
git tag vX.Y.Z
```

本仓库约定：仅以当前 `user.name` 本地提交，绝不推送到远端。

## 4. 版本管理规则

- 单一事实源为 `.version`；包版本由 `build-packages.sh` 推导。
- dev 版本 `X.Y.Z-dev` 对应包 `VERSION=X.Y.Z`、`RELEASE=dev1`；正式版本
  `X.Y.Z` 对应 `RELEASE=1`。
- 同一版本重复打包时通过 `PKG_RELEASE` 递增发布号。
- 发布顺序：先修好版本同步（3.2），再构建与打包，避免带病发包。

## 5. 发行包与校验

| 格式 | 命名示例 | 安装位置 |
|------|----------|----------|
| deb | `memrec_0.11.0-dev1_amd64.deb` | `/usr/bin`、`/usr/lib/systemd/user` |
| rpm | `memrec-0.11.0-dev1.x86_64.rpm` | 同上 |

安装后用户按 postinst 提示执行一次（模型不自动下载）：

```bash
mr-install --skip-install --skip-cargo-check --skip-service
systemctl --user daemon-reload
systemctl --user enable --now memrecd
```

## 6. 发布与归档

- 产物在 `release/` 目录；该目录不纳入 git。
- 如需发布，上传 deb/rpm 与校验信息到 Gitee/GitHub Release（发布动作由发布者手动
  执行，本项目 AI 不执行推送）。
- 保留同一版本的构建来源（commit + tag），便于复现。

## 7. 常见问题

**Q：构建报 `failed to select a version for the requirement ...`？**
版本同步遗漏，见 3.2。请把所有 crate 间路径依赖的版本要求改为当前 workspace
dev 版本。

**Q：deb 安装后 `memrecd` 起不来？**
多为用户尚未下载模型。执行
`mr-install --skip-install --skip-cargo-check --skip-service` 后再
`systemctl --user enable --now memrecd`；查看
`journalctl --user -u memrecd` 与 `~/.memrec/logs/`。

**Q：包管理器装完后系统里有两份 memrecd？**
若用户曾 `cargo install`（`~/.cargo/bin`）或包安装后误用
`mr-install`（默认写 `~/.local/bin`）。包场景务必带 `--skip-install
--skip-service`，并确保 `PATH` 中 `/usr/bin` 生效。

**Q：rpm 校验失败？**
本机可能未安装 `rpm` 工具；`rpm -qip` 需要它。deb 侧用 `dpkg-deb` 即可。
