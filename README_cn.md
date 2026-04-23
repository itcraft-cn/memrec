English | [中文版](README_cn.md)

# MemRec - AI记忆持久化系统

本地化记忆持久化系统，为AI CLI工具（opencode、claude code等）提供跨会话记忆恢复、知识库积累、对话历史存档能力。

## 特性

- **跨会话恢复** - 恢复上下文、偏好、项目知识
- **知识积累** - 存储最佳实践和关键决策
- **对话存档** - 完整对话记录支持检索
- **智能生命周期管理** - 基于重要性评分自动压缩和遗忘
- **混合检索** - 精确+语义检索，RRF融合算法
- **自动拆分** - 长文本（>7.5KB）自动拆分为chunks
- **Systemd集成** - 支持 `systemctl --user` 管理

## 快速开始

### 安装

```bash
# 构建release版本
cargo build --release

# 安装二进制文件
install -m 755 target/release/memrecd ~/.local/bin/
install -m 755 target/release/memrec ~/.local/bin/

# 下载Embedding模型（约90MB）
mkdir -p ~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx
cd ~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/model.onnx
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/tokenizer.json
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/config.json
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/special_tokens_map.json
wget https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main/tokenizer_config.json

# 安装systemd服务（可选）
./scripts/systemd/install.sh
```

**模型配置：**
- 默认路径：`~/.memrec/models/Qdrant--all-MiniLM-L6-v2-onnx/`
- 自定义路径：设置环境变量 `MEMREC_MODEL_DIR`

### 使用

```bash
# 启动守护进程（如未使用systemd）
memrecd

# 添加记忆
memrec add "关键决策" --mtype decision --tag critical
memrec add "最佳实践" --mtype knowledge --tag rust
memrec add "项目配置" --mtype context --tag config

# 检索记忆
memrec list --limit 20
memrec get <id>
memrec stats

# 删除记忆
memrec delete <id>
```

## 记忆类型

- `decision` - 关键决策（推荐用 `critical` 标签）
- `knowledge` - 最佳实践和知识点
- `context` - 项目配置和环境信息
- `preference` - 用户偏好
- `conversation` - 对话记录（默认）

## 记忆管理

自动生命周期管理：
- **重要性评分**：时间衰减 + 访问频率 + 标签权重 + 用户优先级
- **压缩**：低重要性记忆压缩为摘要
- **遗忘**：重要性 < 0.1 且 90天未访问 → 删除

## 启停管理

提供两套管理方式：

### 方式一：手工管理（推荐开发环境）

```bash
# 启动
./scripts/start.sh

# 停止
./scripts/stop.sh

# 重启
./scripts/restart.sh

# 状态
./scripts/status.sh
```

特性：
- PID文件管理
- 后台运行
- 日志输出到 `~/.memrec/memrecd.log`
- 优雅关闭（SIGTERM，10秒超时后强制）

### 方式二：Systemd服务（推荐生产环境）

```bash
# 安装
./scripts/systemd/install.sh

# 管理
systemctl --user start memrecd
systemctl --user stop memrecd
systemctl --user status memrecd
journalctl --user -u memrecd -f
```

或者使用便捷脚本：

```bash
./scripts/memrecctl.sh start
./scripts/memrecctl.sh stop
./scripts/memrecctl.sh status
./scripts/memrecctl.sh logs
```

## Skill集成

AI CLI工具的Skill：`~/.opencode/skills/memrec/SKILL.md`

AI agent可以：
- 自动记录关键决策
- 检索历史知识
- 维护跨会话项目上下文
- 记忆用户偏好

## 文档

- [Systemd指南](docs/systemd.md)
- [设计文档](docs/superpowers/specs/2026-04-23-memrec-design.md)
- [算法文档](docs/superpowers/specs/2026-04-23-memrec-algorithms.md)
- [Skill文档](docs/skills/memrec-skill.md)

## 项目结构

```
memrec/
├── common/       # 共享类型和协议
├── memrecd/      # 守护进程服务
├── memrec/       # CLI工具
└── docs/         # 文档
```

## 许证

MIT

## 更新日志

详见 [CHANGELOG.md](CHANGELOG.md)