# MemRec Daemon — AI 记忆持久化服务器

[![Crates.io](https://img.shields.io/crates/v/memrecd.svg)](https://crates.io/crates/memrecd)
[![文档](https://docs.rs/memrecd/badge.svg)](https://docs.rs/memrecd)
[![许可证](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

MemRec AI 记忆持久化系统的守护进程服务器，提供持久化存储、语义搜索和项目隔离功能。

## 概述

`memrecd` 是 MemRec 生态系统的核心服务器组件。它作为后台守护进程运行，通过 Unix socket 暴露 JSON-RPC 2.0 API，用于记忆操作、语义搜索和项目管理。

## 特性

- **持久化存储**: 基于 RocksDB 的元数据和向量嵌入存储
- **语义搜索**: 使用 ONNX 模型的向量相似性搜索（MiniLM-L6-v2、BGE-M3）
- **项目隔离**: 每个项目独立的记忆空间，支持自动检测
- **Unix Socket API**: 本地通信的 JSON-RPC 2.0 接口
- **重要性评分**: 基于最近性、访问次数和相关性的自动重要性计算
- **分块存储**: 支持大记忆内容的自动分块
- **嵌入生成**: 集成 fastembed 实现高效嵌入

## 安装

### 从 crates.io 安装（推荐）

```bash
cargo install --locked memrecd
```

### 使用 mr-install（一体化安装）

```bash
cargo install --locked mr-install
mr-install
```

## 使用

### 启动守护进程

```bash
# 启动守护进程（将在后台运行）
memrecd

# 启动时使用详细日志
RUST_LOG=debug memrecd

# 检查守护进程状态
systemctl --user status memrecd  # Linux
launchctl list com.itcraft.memrecd  # macOS
```

### 配置

守护进程从 `~/.memrec/config.toml` 读取配置：

```toml
version = "0.3.0"

[model]
model_type = "minilm-l6-v2"  # 或 "bge-m3"
source = "huggingface"
dimension = 384  # BGE-M3 为 1024

[server]
socket_path = "~/.memrec/memrecd.sock"
data_dir = "~/.memrec/data"
vector_dir = "~/.memrec/vectors"
log_path = "~/.memrec/memrecd.log"

# 模型文件及 SHA256 哈希用于安全验证
[[model.files]]
filename = "model.onnx"
sha256 = "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"
required = true

[[model.files]]
filename = "tokenizer.json"
sha256 = "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0"
required = true
```

### 架构

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Unix Socket   │────▶│   JSON-RPC 2.0  │────▶│   请求路由器    │
│     接口        │     │     处理器      │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                                                            │
                                                            ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   向量存储      │◀───▶│   嵌入生成器    │◀───▶│   模型配置      │
│   (RocksDB)     │     │   (fastembed)   │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
        │                        │                        │
        ▼                        ▼                        ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   元数据存储    │     │   项目检测      │     │   重要性        │
│   (RocksDB)     │     │   (.mr_pid)     │     │   计算器        │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

## API 参考

### JSON-RPC 方法

守护进程支持以下 JSON-RPC 2.0 方法：

#### 记忆操作
- `add_memory` - 添加新记忆，支持标签和重要性
- `get_memory` - 按 ID 检索记忆
- `update_memory` - 更新现有记忆
- `delete_memory` - 软删除记忆
- `list_memories` - 分页列出记忆
- `search_memories` - 带相关性评分的语义搜索

#### 项目操作
- `get_project_info` - 获取当前项目信息
- `set_project` - 手动设置项目上下文
- `list_projects` - 列出所有项目

#### 系统操作
- `ping` - 健康检查
- `stats` - 获取服务器统计信息
- `version` - 获取服务器版本

### API 使用示例

```bash
# 使用 curl 与 socket 交互
echo '{
  "jsonrpc": "2.0",
  "id": "1",
  "method": "add_memory",
  "params": {
    "id": "test-123",
    "content": "这是一个测试记忆",
    "mtype": "conversation",
    "tags": ["测试", "示例"],
    "importance": 0.8
  }
}' | socat UNIX-CONNECT:$HOME/.memrec/memrecd.sock STDIO
```

## 开发

### 从源码构建

```bash
# 克隆仓库
git clone https://github.com/itcraft-cn/memrec
cd memrec

# 发布模式构建
cargo build --release --bin memrecd

# 运行测试
cargo test --release --bin memrecd
```

### 运行测试

```bash
# 运行所有测试
cargo test --release

# 运行特定测试类别
cargo test --release --test embedding
cargo test --release --test storage
cargo test --release --test server
```

### 日志记录

守护进程使用 `tracing` 进行结构化日志记录：

```bash
# 不同日志级别
RUST_LOG=error memrecd     # 仅错误
RUST_LOG=warn memrecd      # 警告和错误
RUST_LOG=info memrecd      # 信息级别（默认）
RUST_LOG=debug memrecd     # 调试信息
RUST_LOG=trace memrecd     # 详细跟踪
```

## 性能

### 内存使用
- **元数据**: 每个记忆条目约 50 字节
- **向量**: 每个记忆 384 字节（MiniLM-L6-v2）或 1024 字节（BGE-M3）
- **索引**: 向量索引额外约 20% 开销

### 吞吐量
- **嵌入生成**: CPU 上约 1000 文本/秒
- **搜索**: 最近邻搜索约 10,000 向量/秒
- **存储**: 元数据约 10,000 写入/秒

### 可扩展性
- 支持每个项目数百万条记忆
- 大内容自动分块
- 后台重要性重新计算

## 安全

### 数据保护
- 项目隔离防止跨项目数据访问
- Unix socket 权限限制为所有者访问
- 配置文件仅用户模式权限（600）

### 模型安全
- 下载模型的 SHA256 哈希验证
- 可选的 `--skip-hash-verify` 标志（带安全警告）
- 支持带哈希验证的可信镜像

### 服务安全
- 作为用户服务运行（非 root）
- 默认不暴露网络
- 加固的服务配置文件

## 故障排除

### 常见问题

1. **Socket 连接失败**
   ```bash
   # 检查守护进程是否运行
   ps aux | grep memrecd
   
   # 检查 socket 权限
   ls -la ~/.memrec/memrecd.sock
   
   # 重启守护进程
   systemctl --user restart memrecd
   ```

2. **模型下载失败**
   ```bash
   # 检查网络连接
   curl -I https://huggingface.co
   
   # 使用镜像
   mr-install --use-hf-mirror
   
   # 跳过哈希验证（安全风险）
   mr-install --skip-hash-verify
   ```

3. **存储问题**
   ```bash
   # 检查磁盘空间
   df -h ~/.memrec
   
   # 修复数据库
   rm -rf ~/.memrec/data
   rm -rf ~/.memrec/vectors
   # 重新运行 mr-install 重建
   ```

### 日志
- 服务日志: `~/.memrec/memrecd.log`
- 系统日志: `journalctl --user -u memrecd` (Linux)
- Launchd 日志: `log stream --predicate 'subsystem == "com.itcraft.memrecd"'` (macOS)

## 贡献

开发指南请参阅 [CONTRIBUTING.md](../CONTRIBUTING.md)。

## 许可证

Apache 许可证 2.0 - 详见 [LICENSE](../LICENSE)。

## 链接

- [主仓库](https://github.com/itcraft-cn/memrec)
- [API 文档](https://docs.rs/memrecd)
- [Crates.io](https://crates.io/crates/memrecd)
- [CLI 客户端](../memrec/README.md)
- [安装器](../mr-install/README.md)