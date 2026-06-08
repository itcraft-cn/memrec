# MemRec 安装部署手册

## 系统要求

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux / macOS / Windows |
| Rust | 1.75+ |
| 磁盘空间 | ~200MB (含模型) |
| 内存 | ~150MB (运行时，含模型) |

## 安装目录

| 平台 | 二进制路径 | 数据路径 |
|------|-----------|---------|
| Linux | `~/.local/bin/` | `~/.memrec/` |
| macOS | `~/bin/` | `~/.memrec/` |
| Windows | `%APPDATA%\memrec\` | `~/.memrec/` |

## 快速安装（推荐）

### 1. 构建并安装二进制

```bash
git clone https://github.com/itcraft-cn/memrec.git
cd memrec

cargo build --release
cargo install --path memrec --locked
cargo install --path memrecd --locked
cargo install --path mr-install --locked
```

构建产物安装到 `~/.cargo/bin/`。

### 2. 复制到系统路径

**Linux:**

```bash
cp ~/.cargo/bin/memrec ~/.local/bin/
cp ~/.cargo/bin/memrecd ~/.local/bin/
cp ~/.cargo/bin/mr-install ~/.local/bin/
```

**macOS:**

```bash
mkdir -p ~/bin
cp ~/.cargo/bin/memrec ~/bin/
cp ~/.cargo/bin/memrecd ~/bin/
cp ~/.cargo/bin/mr-install ~/bin/
```

**Windows (PowerShell):**

```powershell
$binDir = "$env:APPDATA\memrec"
New-Item -ItemType Directory -Force -Path $binDir
Copy-Item "$env:USERPROFILE\.cargo\bin\memrec.exe" $binDir
Copy-Item "$env:USERPROFILE\.cargo\bin\memrecd.exe" $binDir
Copy-Item "$env:USERPROFILE\.cargo\bin\mr-install.exe" $binDir
```

### 3. 一键配置

```bash
mr-install
```

`mr-install` 自动完成：
1. 创建 `~/.memrec/` 目录结构（data / vectors / models / logs）
2. 生成 `~/.memrec/config.toml` 默认配置
3. 下载 Embedding 模型（~90MB）
4. 注册并启动守护进程服务
5. 验证安装（写入/搜索/删除测试）

#### 模型下载镜像

默认从 HuggingFace 下载，失败时自动回退 hf-mirror.com。也可手动指定：

```bash
# 直接使用 hf-mirror.com
mr-install --use-hf-mirror

# 使用自定义镜像
mr-install --mirror-base-url https://your-mirror.example.com
```

#### 跳过步骤

```bash
mr-install --skip-model    # 跳过模型下载
mr-install --skip-service  # 跳过服务注册
mr-install --skip-verify   # 跳过安装验证
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

### Windows (Startup)

Windows 通过 Startup 文件夹中的 VBS 脚本启动 memrecd：

- 启动脚本：`~/.memrec/start_memrecd.ps1`
- 启动快捷方式：`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\memrecd.vbs`
- `mr-install` 已自动将 `%APPDATA%\memrec` 注册到用户 PATH 环境变量

手动管理：

```powershell
# 启动
powershell -ExecutionPolicy Bypass -File "$env:USERPROFILE\.memrec\start_memrecd.ps1"

# 停止
taskkill /IM memrecd.exe /F

# 查看状态
tasklist /FI "IMAGENAME eq memrecd.exe"
```

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
cd memrec
git pull

# 重新构建并安装
cargo build --release
cargo install --path memrec --locked
cargo install --path memrecd --locked

# 复制到系统路径（Linux）
cp ~/.cargo/bin/memrec ~/.local/bin/
cp ~/.cargo/bin/memrecd ~/.local/bin/

# 重启服务
mr-install --skip-model --skip-verify

# 验证版本
memrec version
```

## 卸载

```bash
# 停止并注销服务
mr-install --skip-model --skip-verify
# 然后手动：停止服务、删除服务文件、删除二进制、删除数据

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

# Windows
taskkill /IM memrecd.exe /F
del "%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\memrecd.vbs"
del "%APPDATA%\memrec\memrec.exe" "%APPDATA%\memrec\memrecd.exe" "%APPDATA%\memrec\mr-install.exe"
# 从用户 PATH 移除 %APPDATA%\memrec

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

### Q: Windows 上 memrec 命令找不到

1. 确认 `%APPDATA%\memrec` 已添加到用户 PATH（mr-install 自动完成）
2. 新开终端窗口使 PATH 生效
3. 手动添加：`[Environment]::SetEnvironmentVariable('Path', "$([Environment]::GetEnvironmentVariable('Path', 'User'));$env:APPDATA\memrec", 'User')`

### Q: 如何更换 Embedding 模型

1. 下载新模型到 `~/.memrec/models/` 下的新目录
2. 设置 `MEMREC_MODEL_DIR` 指向新模型
3. 重启服务
4. 注意：更换模型后需重建向量索引（删除 `~/.memrec/vectors/` 后重启）
