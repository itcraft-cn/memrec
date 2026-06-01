[English](README.md) | 中文

# MemRec — AI记忆持久化系统

> 本地优先的AI记忆，项目隔离 — 为终端而生，为私密而用

本地化记忆持久化系统，为AI CLI工具提供跨会话记忆恢复、知识积累、对话存档能力。

## 特性

- **项目隔离** — 自动检测git root，.mr_pid持久化，多项目独立记忆
- **语义检索** — 本地ONNX模型（384维），零API费用，数据全本地
- **跨项目搜索** — `--all`标志发现跨项目关联知识
- **AI-first设计** — 默认JSON输出，命令简洁，Skill集成
- **高性能** — Rust实现，<1ms延迟，~118MB内存（含模型）
- **长文本拆分** — >7.5KB自动拆分为chunks
- **Systemd集成** — `systemctl --user`管理守护进程

## 快速开始

### 安装

```bash
cargo build --release
cargo install --path memrec --locked
cargo install --path memrecd --locked
cp ~/.cargo/bin/memrec ~/.local/bin/
cp ~/.cargo/bin/memrecd ~/.local/bin/
```

### 下载模型

```bash
mkdir -p ~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx
cd ~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/model.onnx
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/tokenizer.json
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/config.json
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/special_tokens_map.json
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/tokenizer_config.json
```

### 启动服务

```bash
# 前台启动（调试）
memrecd

# 或 systemd 服务（推荐）
systemctl --user enable memrecd
systemctl --user start memrecd
```

### 使用

```bash
# 添加记忆
memrec add "选择JWT认证方案" --mtype decision --tag critical
memrec add "RAII模式：资源获取即初始化" --mtype knowledge --tag best-practice --tag rust
memrec add "用户偏好详细输出" --mtype preference --tag output --global

# 语义检索
memrec search "认证方案"                   # 当前项目 + 公共
memrec search "性能优化" --project-only    # 仅当前项目
memrec search "用户偏好" --global-only     # 仅公共记忆
memrec search "xlsb" --all                 # 跨所有项目

# 其他
memrec list --limit 20
memrec get <id>
memrec stats
memrec version
```

## 项目隔离

MemRec 自动为不同项目创建独立的记忆空间：

```
project-a/           project-b/
├── .mr_pid          ├── .mr_pid        ← 自动创建，不同ID
├── .gitignore       ├── .gitignore     ← 建议添加 .mr_pid
└── src/             └── src/
```

- **git仓库**：自动检测 git root
- **非git目录**：使用当前工作目录
- **公共记忆**：`--global` 标记，所有项目可检索
- **跨项目搜索**：`--all` 搜索所有项目

## 记忆类型

| 类型 | 标识 | 用途 |
|------|------|------|
| 决策 | `decision` | 关键技术/业务决策 |
| 知识 | `knowledge` | 知识点（通过tag细分：`fact`/`best-practice`/`algorithm`/`tool`） |
| 上下文 | `context` | 项目配置、环境信息 |
| 偏好 | `preference` | 用户偏好（推荐 `--global`） |
| 对话 | `conversation` | 对话记录（默认） |

## 数据位置

```
~/.memrec/
├── memrecd.sock        # Unix Socket
├── data/               # RocksDB 记忆元数据
├── vectors/            # RocksDB 向量存储
└── models/             # ONNX Embedding 模型
```

## 环境变量

| 变量 | 用途 | 默认值 |
|------|------|--------|
| `MEMREC_MODEL_DIR` | 自定义模型路径 | `~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx/` |
| `MEMREC_MIN_SCORE` | 语义搜索最低相似度 | `0.75` |
| `RUST_LOG` | 日志级别 | `info` |

## 文档

- [安装部署手册](docs/installation.md)
- [使用手册](docs/user-guide.md)
- [Systemd指南](docs/systemd.md)
- [Skill文档](docs/skills/memrec-skill.md)

## 项目结构

```
memrec/
├── common/       # 共享类型和协议
├── memrecd/      # 守护进程服务
├── memrec/       # CLI工具
└── docs/         # 文档
```

## 许可证

Apache-2.0

## 更新日志

详见 [CHANGELOG.md](CHANGELOG.md)
