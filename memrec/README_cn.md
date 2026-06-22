# MemRec CLI — AI 记忆持久化客户端

[![Crates.io](https://img.shields.io/crates/v/memrec.svg)](https://crates.io/crates/memrec)
[![文档](https://docs.rs/memrec/badge.svg)](https://docs.rs/memrec)
[![许可证](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

MemRec AI 记忆持久化系统的命令行界面，提供直观的记忆存储、语义搜索和项目管理访问。

## 概述

`memrec` 是与 MemRec 守护进程交互的主要 CLI 客户端。它提供了一套全面的命令，用于管理具有项目隔离、语义搜索功能以及与 AI 工具和工作流程无缝集成的 AI 记忆。

## 特性

- **项目感知**: 通过 `.mr_pid` 文件自动检测项目上下文
- **语义搜索**: 按含义而非关键词查找记忆
- **记忆管理**: 添加、检索、更新和删除记忆
- **重要性评分**: 自动相关性排名，支持手动调整
- **标签系统**: 灵活的组织和发现标签
- **JSON 输出**: AI 友好的默认输出格式，支持 `--human` 标志
- **批量操作**: 支持处理多个记忆
- **MCP 支持**: Model Context Protocol 集成，支持 AI 工具

## 安装

### 从 crates.io 安装（推荐）

```bash
cargo install --locked memrec
```

### 使用 mr-install（一体化安装）

```bash
cargo install --locked mr-install
mr-install
```

## 快速开始

```bash
# 首先，确保守护进程正在运行
memrecd &

# 添加第一条记忆（自动检测项目上下文）
memrec add meeting-notes --mtype conversation \
  --content "讨论了项目架构并决定采用微服务" \
  --tags 架构 会议 规划

# 搜索相关记忆
memrec search "微服务架构"

# 列出当前项目中的所有记忆
memrec list

# 获取特定记忆
memrec get meeting-notes
```

## 使用

### 基本命令

#### 记忆操作

```bash
# 添加新记忆
memrec add <id> --mtype <类型> --content <文本> [--tags <标签1,标签2>] [--importance <0.0-1.0>]

# 示例：
memrec add api-design --mtype code --content "REST 服务的 API 设计模式" --tags api 设计 模式
memrec add bug-fix --mtype fix --content "修复了向量存储中的内存泄漏" --tags 修复 性能 --importance 0.9

# 按 ID 获取记忆
memrec get <id>

# 更新记忆
memrec update <id> --content <新文本> [--tags <新标签>]

# 删除记忆（软删除）
memrec delete <id>

# 分页列出记忆
memrec list [--limit <数量>] [--offset <偏移>] [--order-by <字段>] [--asc|--desc]
```

#### 搜索操作

```bash
# 语义搜索（向量相似性）
memrec search <查询> [--limit <数量>] [--min-score <0.0-1.0>]

# 按标签搜索
memrec search --tags <标签1,标签2> [--limit <数量>]

# 组合搜索
memrec search "数据库优化" --tags 性能 sql --limit 10

# 跨所有项目搜索
memrec search --global "通用模式"
```

#### 项目操作

```bash
# 获取当前项目信息
memrec project

# 手动设置项目
memrec project set <项目ID>

# 列出所有项目
memrec project list

# 创建新项目
memrec project create <名称> [--description <描述>] [--tags <标签1,标签2>]
```

### 高级用法

#### 导入/导出

```bash
# 从 JSON 文件导入
memrec import --file memories.json [--project <项目ID>]

# 导出记忆到 JSON
memrec export --file backup.json [--project <项目ID>]

# 带过滤器的导出
memrec export --file important.json --min-importance 0.7 --tags 重要
```

#### 批量操作

```bash
# 从标准输入批量添加（JSON 行）
cat memories.jsonl | memrec batch add

# 批量更新
echo '{"id": "mem1", "content": "已更新"}' | memrec batch update

# 批量删除
echo '["mem1", "mem2"]' | memrec batch delete
```

#### 重要性管理

```bash
# 重新计算所有记忆的重要性
memrec importance recalc

# 获取重要性统计信息
memrec importance stats

# 设置手动重要性覆盖
memrec set-importance <id> <0.0-1.0>
```

## 输出格式

### JSON（AI 工具默认）

```bash
# 默认 JSON 输出
memrec get meeting-notes
# 输出：{"id": "meeting-notes", "content": "...", "importance": 0.85, ...}

# 美化 JSON
memrec get meeting-notes --json-pretty
```

### 人类可读

```bash
# 人类可读表格格式
memrec list --human

# 指定列的人类可读格式
memrec list --human --columns id,content,importance,tags

# 彩色输出
memrec search "模式" --human --color
```

### 机器格式

```bash
# CSV 输出
memrec list --format csv

# TSV 输出  
memrec list --format tsv

# YAML 输出
memrec get meeting-notes --format yaml
```

## 项目检测

MemRec 自动检测项目上下文：

1. **`.mr_pid` 文件**: 在当前目录或父目录中查找 `.mr_pid`
2. **Git 仓库**: 回退到 git 仓库根目录作为项目
3. **主目录**: 使用 `~/.memrec` 作为全局记忆
4. **手动覆盖**: 使用 `memrec project set <id>` 覆盖

创建项目标识符：

```bash
# 在项目根目录中
echo "my-awesome-project" > .mr_pid
# 现在所有 memrec 命令都将使用此项目上下文
```

## 集成示例

### 与 AI CLI 工具集成

```bash
# 存储 AI 对话历史
ai --model gpt-4 "解释微服务" | \
  memrec add ai-explanation --mtype conversation \
  --content "$(cat)" --tags ai 微服务 解释

# 在询问 AI 前搜索相关上下文
context=$(memrec search "数据库架构" --limit 3 --format content-only)
ai --model claude "设计一个数据库架构。上下文：$context"
```

### 在 Shell 脚本中使用

```bash
#!/bin/bash
# 将命令输出存储为记忆
memrec add "cmd-output-$(date +%s)" --mtype log \
  --content "$(some-command 2>&1)" \
  --tags 脚本 $(basename $0)

# 搜索故障排除信息
if memrec search "错误 $(some-command)" --min-score 0.8; then
  echo "在记忆中找到了类似错误"
fi
```

### 与 MCP（Model Context Protocol）集成

```bash
# MCP 服务器将记忆作为上下文提供给 AI 模型
memrec mcp-server

# 在 AI 工具配置中：
# {
#   "mcpServers": {
#     "memrec": {
#       "command": "memrec",
#       "args": ["mcp-server"]
#     }
#   }
# }
```

## 配置

### 环境变量

```bash
# Socket 路径覆盖
export MEMREC_SOCKET_PATH="/custom/path/memrecd.sock"

# 最小搜索分数
export MEMREC_MIN_SCORE=0.75

# 默认输出格式
export MEMREC_OUTPUT_FORMAT="human"

# 模型目录
export MEMREC_MODEL_DIR="$HOME/.memrec/models"

# 日志级别
export RUST_LOG="info"
```

### 配置文件

创建 `~/.memrec/cli_config.toml`：

```toml
[defaults]
output_format = "json"  # 或 "human"
color = true
confirm_deletes = true

[search]
default_limit = 10
min_score = 0.75
include_global = false

[project]
auto_detect = true
fallback_to_git = true

[formatting]
date_format = "%Y-%m-%d %H:%M:%S"
truncate_content = 200
```

## 性能提示

### 内存使用
- 使用简洁但描述性的内容以获得更好的嵌入
- 添加相关标签以提高搜索准确性
- 为频繁访问的记忆设置适当的重要性分数

### 搜索优化
- 使用特定查询而非通用术语
- 将语义搜索与标签过滤器结合使用
- 根据用例调整 `--min-score`（默认 0.75）

### 存储管理
- 定期清理低重要性记忆
- 使用 `memrec importance recalc` 保持准确性
- 清理前导出重要记忆

## 故障排除

### 常见问题

```bash
# 守护进程未运行
错误：无法连接到 socket
解决方案：使用 `memrecd` 启动守护进程或检查服务状态

# 未检测到项目  
警告：未检测到项目上下文
解决方案：创建 `.mr_pid` 文件或使用 `memrec project set`

# 搜索无结果
# 尝试：调整 --min-score、添加更具体的术语或检查嵌入模型

# 权限被拒绝
错误：权限被拒绝（os error 13）
解决方案：检查 socket 权限或以正确用户身份运行
```

### 调试模式

```bash
# 启用调试输出
RUST_LOG=debug memrec <命令>

# 跟踪所有操作
RUST_LOG=trace memrec <命令> --verbose
```

## 贡献

开发指南请参阅 [CONTRIBUTING.md](../CONTRIBUTING.md)。

## 许可证

Apache 许可证 2.0 - 详见 [LICENSE](../LICENSE)。

## 链接

- [主仓库](https://github.com/itcraft-cn/memrec)
- [API 文档](https://docs.rs/memrec)
- [Crates.io](https://crates.io/crates/memrec)
- [守护进程服务器](../memrecd/README.md)
- [安装器](../mr-install/README.md)