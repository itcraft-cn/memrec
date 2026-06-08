# MemRec 安装部署手册

## 系统要求

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux / macOS |
| Rust | 1.75+ (仅 mr-install 首次安装需要) |
| 磁盘空间 | ~200MB (含模型) |
| 内存 | ~150MB (运行时，含模型) |

## 安装目录

| 平台 | 二进制路径 | 数据路径 |
|------|-----------|---------|
| Linux | `~/.local/bin/` | `~/.memrec/` |
| macOS | `~/bin/` | `~/.memrec/` |

## 一键安装（推荐）

```bash
cargo install --locked mr-install
mr-install
```

`mr-install` 自动完成：
1. **安装二进制** — 通过 `cargo install` 编译安装 memrec/memrecd/mr-install，复制到系统路径
2. **创建目录** — `~/.memrec/` 目录结构（data / vectors / models / logs）
3. **生成配置** — `~/.memrec/config.toml` 默认配置
4. **下载模型** — ONNX Embedding 模型（~90MB），HuggingFace 失败自动回退 hf-mirror.com
5. **注册服务** — 守护进程自动启动
6. **验证安装** — 写入/搜索/删除测试

### 模型下载镜像

```bash
# 直接使用 hf-mirror.com（中国大陆推荐）
mr-install --use-hf-mirror

# 使用自定义镜像
mr-install --mirror-base-url https://your-mirror.example.com
```

### 自定义仓库源

```bash
# 从 Gitee 安装
mr-install --repo-url https://gitee.com/itcraft-cn/memrec
```

### 跳过步骤

```bash
mr-install --skip-install   # 跳过 cargo install（已手动安装二进制）
mr-install --skip-model     # 跳过模型下载
mr-install --skip-service   # 跳过服务注册
mr-install --skip-verify    # 跳过安装验证
```

## 服务管理

### Linux (systemd)

```bash
systemctl --user status memrecd     # 查看状态
systemctl --user stop memrecd       # 停止
systemctl --user restart memrecd    # 重启
journalctl --user -u memrecd -f     # 查看日志
```

服务文件：`~/.config/systemd/user/memrecd.service`

### macOS (launchd)

```bash
launchctl list com.itcraft.memrecd               # 查看状态
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.itcraft.memrecd.plist   # 停止
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.itcraft.memrecd.plist # 启动
```

配置文件：`~/Library/LaunchAgents/com.itcraft.memrecd.plist`

特性：RunAtLoad + KeepAlive，登录即启动，崩溃自动重启。

## 数据目录结构

```
~/.memrec/
├── config.toml           # 配置文件
├── memrecd.sock          # Unix Socket（运行时生成，Linux/macOS）
├── memrecd.log           # 服务日志
├── data/                 # RocksDB 记忆元数据
├── vectors/              # RocksDB 向量存储
├── models/               # ONNX Embedding 模型
│   └── Qdrant--all-MiniLM-L6-v2-onnx/
│       ├── model.onnx
│       ├── tokenizer.json
│       ├── config.json
│       ├── special_tokens_map.json
│       └── tokenizer_config.json
└── logs/                 # 日志目录
```

## 项目目录结构

MemRec 使用 `.mr_pid` 文件实现项目隔离。在项目根目录下会自动生成：

```
your-project/
├── .mr_pid               # 项目ID文件（勿提交git）
├── .gitignore            # 建议添加 .mr_pid
└── ...
```

**重要：** 将 `.mr_pid` 添加到 `.gitignore`，避免多人项目ID冲突。

## 环境变量

| 变量 | 用途 | 默认值 |
|------|------|--------|
| `MEMREC_MODEL_DIR` | 自定义模型路径 | `~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx/` |
| `MEMREC_MIN_SCORE` | 语义搜索最低相似度 | `0.75` |
| `RUST_LOG` | 日志级别 | `info` |

## 升级

```bash
mr-install
```

重新运行 mr-install 即可升级（cargo install 会覆盖旧版本，服务会重启）。

## 卸载

```bash
# Linux
systemctl --user stop memrecd
systemctl --user disable memrecd
rm ~/.config/systemd/user/memrecd.service
systemctl --user daemon-reload
rm ~/.local/bin/memrec ~/.local/bin/memrecd ~/.local/bin/mr-install

# macOS
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.itcraft.memrecd.plist
rm ~/Library/LaunchAgents/com.itcraft.memrecd.plist
rm ~/bin/memrec ~/bin/memrecd ~/bin/mr-install

# 删除数据（可选，会清除所有记忆）
rm -rf ~/.memrec
```

## 常见问题

### Q: 启动报错 "Failed to connect to memrecd"

守护进程未运行。执行 `memrecd` 或使用服务管理命令启动。

### Q: 语义搜索返回0条结果

1. 检查模型文件是否完整：`ls ~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx/`
2. 检查服务日志：`cat ~/.memrec/memrecd.log`
3. 降低 min_score 阈值：`memrec search "query" --min-score 0.5`

### Q: 项目记忆没有隔离

1. 确认 `.mr_pid` 文件存在于项目根目录
2. 确认在项目目录内执行命令（不是 `~` 或 `/tmp`）
3. git 仓库会自动检测 git root

### Q: 模型下载失败（中国大陆网络）

```bash
mr-install --use-hf-mirror
```

### Q: 如何更换 Embedding 模型

1. 下载新模型到 `~/.memrec/models/` 下的新目录
2. 设置 `MEMREC_MODEL_DIR` 指向新模型
3. 重启服务
4. 注意：更换模型后需重建向量索引（删除 `~/.memrec/vectors/` 后重启）
