# 打包说明（deb/rpm，nfpm 工具链）

本目录描述 MemRec Linux 发行包的构建方式（主选方案：nfpm）。
macOS dmg 计划单独实现，见文末「后续计划」。

## 目录结构

```
packaging/
├── nfpm.yaml                 # nfpm 打包配置（deb + rpm 共用）
├── systemd/
│   └── memrecd.service       # 随包安装的 systemd 用户服务单元
├── scripts/
│   ├── postinstall.sh        # deb postinst / rpm %post：打印后续指引
│   └── postremove.sh         # deb postrm / rpm %postun：提示数据保留与清理
└── README.md                 # 本文档
```

构建入口不在本目录，而是 `scripts/package/`：

```
scripts/package/
├── build-packages.sh         # 解析版本 -> 构建二进制 -> 调用 nfpm -> 归档 release/
└── ensure-nfpm.sh            # 定位 nfpm（系统优先，缺失则下载到 scripts/tools/bin/）
```

## 打包模型

- 系统级安装：二进制装到 `/usr/bin`；
- 用户级运行：服务为 systemd 用户单元 `/usr/lib/systemd/user/memrecd.service`，
  数据与模型位于各用户的 `~/.memrec/`；
- 模型下载不在包安装阶段自动进行（模型数 GB），由用户执行一次
  `mr-install --skip-install --skip-cargo-check --skip-service`；
- 卸载不删除任何用户数据。

## 版本与发布号

版本唯一事实源为仓库根 `.version`（其次为 `Cargo.toml` 的 `[workspace.package].version`）。
`build-packages.sh` 按如下规则拆分：

| 事实源版本 | 包 VERSION | 包 RELEASE | 结果（deb / rpm） |
|-----------|-----------|-----------|------------------|
| `0.11.0-dev` | `0.11.0` | `dev1` | `memrec_0.11.0-dev1_amd64.deb` / `memrec-0.11.0-dev1.x86_64.rpm` |
| `0.11.0` | `0.11.0` | `1` | `memrec_0.11.0-1_amd64.deb` / `memrec-0.11.0-1.x86_64.rpm` |

可用 `PKG_VERSION` / `PKG_RELEASE` 覆盖，便于同版本重打包（递增 release）。

## 使用

```bash
# 首次需仓库可 release 编译
scripts/package/build-packages.sh              # deb + rpm
scripts/package/build-packages.sh deb          # 仅 deb
scripts/package/build-packages.sh rpm          # 仅 rpm
SKIP_BUILD=1 scripts/package/build-packages.sh # 复用已有 target/release 产物
```

产物统一写入仓库根的 `release/` 目录（已加入 `.gitignore`）。

依赖 nfpm：脚本优先使用系统 `nfpm`；缺失时自动下载官方预编译二进制
（版本由 `NFPM_VERSION` 控制，默认 2.48.0）到 `scripts/tools/bin/`。

## 校验

```bash
# deb
dpkg-deb -I release/*.deb        # 控制信息（版本、依赖、维护者、脚本）
dpkg-deb -c release/*.deb        # 文件清单
dpkg-deb -x release/*.deb /tmp/x # 解包到临时目录

# rpm（需 rpm 工具）
rpm -qip release/*.rpm           # 控制信息
rpm -qlp release/*.rpm           # 文件清单

# 服务单元语法（系统需能看到 /usr/bin/memrecd）
systemd-analyze verify packaging/systemd/memrecd.service
```

运行时依赖与 `ldd` 结果一致：deb 为 `libc6 / libgcc-s1 / libstdc++6 / libssl3 /
zlib1g / libzstd1`，rpm 为 `glibc / libgcc / libstdc++ / openssl-libs / zlib /
libzstd`。ONNX Runtime 已静态链接，无额外动态依赖。

## 后续计划

- macOS dmg：使用 `create-dmg`（或 `hdiutil` + AppleScript）打包 `.app`/可执行文件，
  另建分支与脚本；服务使用 launchd（`mr-install` 已实现 launchd）。
- 可选：arch linux 的 pacman 包（nfpm 原生支持）。
