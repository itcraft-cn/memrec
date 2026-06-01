# MemRec 安装部署手册

## 系统要求

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux (已验证), macOS (待验证) |
| Rust | 1.75+ |
| 磁盘空间 | ~200MB (含模型) |
| 内存 | ~150MB (运行时，含模型) |

## 从源码构建

### 1. 克隆仓库

```bash
git clone https://github.com/yourname/memrec.git
cd memrec
```

### 2. 构建

```bash
cargo build --release
```

构建产物位于 `target/release/`：
- `memrecd` — 守护进程（约41MB）
- `memrec` — CLI工具（约2.4MB）

### 3. 安装二进制文件

**方式A：cargo install（推荐）**

```bash
cargo install --path memrec --locked
cargo install --path memrecd --locked
```

安装到 `~/.cargo/bin/`。

**方式B：手动复制**

```bash
install -m 755 target/release/memrecd ~/.local/bin/
install -m 755 target/release/memrec ~/.local/bin/
```

### 4. 下载 Embedding 模型

MemRec 使用 ONNX 格式的本地模型进行语义检索，无需网络连接。

```bash
mkdir -p ~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx
cd ~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx

wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/model.onnx
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/tokenizer.json
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/config.json
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/special_tokens_map.json
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/tokenizer_config.json
```

模型信息：
- 名称：all-MiniLM-L6-v2
- 维度：384
- 大小：约90MB
- 默认路径：`~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx/`

**自定义模型路径：**

```bash
export MEMREC_MODEL_DIR=/path/to/your/model
```

### 5. 验证安装

```bash
memrecd --help
memrec --help
```

## 启动守护进程

### 方式一：前台启动（调试用）

```bash
memrecd
```

按 Ctrl+C 停止。

### 方式二：systemd 服务（推荐）

#### 安装服务

```bash
# 创建 systemd 用户目录
mkdir -p ~/.config/systemd/user

# 创建服务文件
cat > ~/.config/systemd/user/memrecd.service << 'EOF'
[Unit]
Description=MemRec Memory Persistence Daemon
Documentation=https://github.com/yourname/memrec
After=default.target

[Service]
Type=simple
ExecStart=%h/.local/bin/memrecd
ExecStopPost=/bin/rm -f %h/.memrec/memrecd.sock
Restart=on-failure
RestartSec=5

Environment="RUST_LOG=info"
WorkingDirectory=%h/.memrec

StandardOutput=append:%h/.memrec/memrecd.log
StandardError=append:%h/.memrec/memrecd.log

[Install]
WantedBy=default.target
EOF

# 重载并启动
systemctl --user daemon-reload
systemctl --user enable memrecd
systemctl --user start memrecd
```

#### 管理服务

```bash
systemctl --user status memrecd     # 查看状态
systemctl --user stop memrecd       # 停止
systemctl --user restart memrecd    # 重启
journalctl --user -u memrecd -f     # 查看日志
```

### 方式三：脚本管理

```bash
./scripts/start.sh    # 启动
./scripts/stop.sh     # 停止
./scripts/restart.sh  # 重启
./scripts/status.sh   # 状态
```

## 数据目录结构

安装完成后，`~/.memrec/` 目录结构如下：

```
~/.memrec/
├── memrecd.sock          # Unix Socket（运行时生成）
├── memrecd.log           # 服务日志
├── data/                 # RocksDB 记忆元数据
│   ├── *.sst
│   ├── CURRENT
│   ├── LOCK
│   └── ...
├── vectors/              # RocksDB 向量存储
│   ├── *.sst
│   ├── CURRENT
│   ├── LOCK
│   └── ...
└── models/               # ONNX Embedding 模型
    └── Qdrant--all-MiniLM-L6-v2-onnx/
        ├── model.onnx
        ├── tokenizer.json
        ├── config.json
        ├── special_tokens_map.json
        └── tokenizer_config.json
```

## 项目目录结构

MemRec 使用 `.mr_pid` 文件实现项目隔离。在项目根目录下会自动生成：

```
your-project/
├── .mr_pid               # 项目ID文件（勿提交git）
├── .gitignore            # 建议添加 .mr_pid
└── ...
```

`.mr_pid` 文件内容示例：

```
memrec_project_id=b435a636-481b-43dd-a819-cc2cedebf365
created_at=2026-04-23T02:45:47.828915366+00:00
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

# 重新构建
cargo build --release

# 重新安装
cargo install --path memrec --locked
cargo install --path memrecd --locked

# 复制到local bin
cp ~/.cargo/bin/memrec ~/.local/bin/
cp ~/.cargo/bin/memrecd ~/.local/bin/

# 重启服务
systemctl --user restart memrecd

# 验证版本
memrec version
```

## 卸载

```bash
# 停止服务
systemctl --user stop memrecd
systemctl --user disable memrecd
rm ~/.config/systemd/user/memrecd.service
systemctl --user daemon-reload

# 删除二进制
rm ~/.local/bin/memrec ~/.local/bin/memrecd

# 删除数据（可选，会清除所有记忆）
rm -rf ~/.memrec
```

## 常见问题

### Q: 启动报错 "Failed to connect to memrecd"

守护进程未运行。执行 `memrecd` 或 `systemctl --user start memrecd`。

### Q: 语义搜索返回0条结果

1. 检查模型文件是否完整：`ls ~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx/`
2. 检查服务日志：`cat ~/.memrec/memrecd.log`
3. 降低 min_score 阈值：`memrec search "query" --min-score 0.5`

### Q: 项目记忆没有隔离

1. 确认 `.mr_pid` 文件存在于项目根目录
2. 确认在项目目录内执行命令（不是 `~` 或 `/tmp`）
3. git 仓库会自动检测 git root

### Q: 如何更换 Embedding 模型

1. 下载新模型到 `~/.memrec/models/` 下的新目录
2. 设置 `MEMREC_MODEL_DIR` 指向新模型
3. 重启服务
4. 注意：更换模型后需重建向量索引（删除 `~/.memrec/vectors/` 后重启）
