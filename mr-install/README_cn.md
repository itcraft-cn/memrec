# MemRec 安装器 — 一站式安装工具

[![Crates.io](https://img.shields.io/crates/v/mr-install.svg)](https://crates.io/crates/mr-install)
[![许可证](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

MemRec AI 记忆持久化系统的一站式安装工具，提供安全、平台特定的安装服务，包含自动服务配置。

## 概述

`mr-install` 是安装完整 MemRec 生态系统的推荐方式。它处理从下载依赖、安装二进制文件、设置服务到为最佳性能和安全性配置系统的所有工作。

## 特性

- **单命令安装**: 一个命令完成完整设置
- **平台支持**: Linux (systemd) 和 macOS (launchd) 服务
- **安全加固**: SHA256 哈希验证，Git 仓库白名单
- **自动服务管理**: 后台守护进程设置，支持自动重启
- **模型下载**: 自动嵌入模型下载，支持镜像回退
- **配置生成**: 自动创建 `~/.memrec/config.toml`
- **验证测试**: 安装后验证确保一切正常工作
- **卸载支持**: 清理移除所有组件

## 快速开始

```bash
# 从 crates.io 安装 mr-install
cargo install --locked mr-install

# 运行安装器（首次设置推荐）
mr-install

# 替代方案：直接安装，无需单独步骤
cargo install --locked mr-install && mr-install
```

## 安装方法

### 标准安装（推荐）

```bash
mr-install
```

此操作执行：
1. ✅ 通过 `cargo install` 安装二进制文件
2. ✅ 模型下载（MiniLM-L6-v2 约 90MB）
3. ✅ 服务注册（systemd/launchd）
4. ✅ 配置生成
5. ✅ 验证测试

### 自定义安装选项

```bash
# 使用 HuggingFace 镜像（中国用户）
mr-install --use-hf-mirror

# 自定义 Git 仓库（高级用户）
mr-install --repo-url "https://gitee.com/itcraft-cn/memrec"

# 跳过验证测试（开发）
mr-install --skip-verify

# 跳过哈希验证（安全风险 - 不推荐）
mr-install --skip-hash-verify

# 允许任意 Git 仓库（安全风险 - 仅开发）
mr-install --allow-any-repo --repo-url "https://example.com/custom-repo"
```

### 平台特定详情

#### Linux (systemd)

```bash
# 安装目录
~/.local/bin/memrec        # CLI 客户端
~/.local/bin/memrecd       # 守护进程服务器
~/.local/bin/mr-install    # 安装器本身

# 服务文件
~/.config/systemd/user/memrecd.service

# 手动服务控制
systemctl --user status memrecd
systemctl --user start memrecd
systemctl --user stop memrecd
systemctl --user enable memrecd   # 登录时自动启动
```

#### macOS (launchd)

```bash
# 安装目录
~/bin/memrec               # CLI 客户端
~/bin/memrecd              # 守护进程服务器
~/bin/mr-install           # 安装器本身

# 服务文件
~/Library/LaunchAgents/com.itcraft.memrecd.plist

# 手动服务控制
launchctl list com.itcraft.memrecd
launchctl start com.itcraft.memrecd
launchctl stop com.itcraft.memrecd
launchctl bootstrap gui/$UID ~/Library/LaunchAgents/com.itcraft.memrecd.plist
```

## 安全特性

### 模型完整性验证

```bash
# 所有模型文件的 SHA256 哈希验证
const MODEL_HASHES: &[(&str, &str)] = &[
    ("model.onnx", "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"),
    ("tokenizer.json", "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0"),
    # ... 更多文件
];

# 验证过程：
# 1. 下载文件
# 2. 计算 SHA256 哈希
# 3. 与预期哈希比较
# 4. 不匹配时重新下载
# 5. 仅在使用明确的 --skip-hash-verify 时跳过
```

### Git 仓库白名单

```rust
const ALLOWED_GIT_REPOS: &[&str] = &[
    "https://github.com/itcraft-cn/memrec",    # 官方
    "https://gitee.com/itcraft-cn/memrec",     # 镜像
];

// 默认只允许这些仓库
// 使用 --allow-any-repo 绕过（安全风险）
```

### 下载源和回退

```bash
# 主要源（默认）
https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx

# 自动回退（如果主要源失败）
https://hf-mirror.com/Qdrant/all-MiniLM-L6-v2-onnx

# 自定义镜像
mr-install --mirror-base-url "https://custom-mirror.example.com"
```

## 安装过程详情

### 步骤 1: 二进制安装

```bash
# 通过 cargo install --locked 安装
cargo install --locked memrec-common
cargo install --locked memrecd
cargo install --locked memrec
cargo install --locked mr-install

# 选项：
# --repo-url: 自定义 Git 仓库（强制执行白名单）
# --allow-any-repo: 禁用白名单（安全风险）
```

### 步骤 2: 目录设置

```bash
# 创建必要目录
~/.memrec/
├── models/                    # 嵌入模型
│   └── Qdrant--all-MiniLM-L6-v2-onnx/
├── data/                      # 元数据存储 (RocksDB)
├── vectors/                   # 向量存储 (RocksDB)
├── config.toml                # 配置文件
└── memrecd.log               # 守护进程日志文件
```

### 步骤 3: 模型下载

下载嵌入模型（约 90MB），包含：
- 每个文件的进度条
- 哈希验证 (SHA256)
- 主要源 + 回退镜像
- 现有文件的恢复能力

### 步骤 4: 服务注册

#### Linux (systemd)
```ini
[Unit]
Description=MemRec 记忆持久化守护进程
Documentation=https://github.com/itcraft-cn/memrec
After=default.target

[Service]
Type=simple
ExecStart=/home/user/.local/bin/memrecd
ExecStopPost=/bin/rm -f /home/user/.memrec/memrecd.sock
Restart=on-failure
RestartSec=5
Environment="RUST_LOG=info"
WorkingDirectory=/home/user/.memrec
StandardOutput=append:/home/user/.memrec/memrecd.log
StandardError=append:/home/user/.memrec/memrecd.log

[Install]
WantedBy=default.target
```

#### macOS (launchd)
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.itcraft.memrecd</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/user/bin/memrecd</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>StandardOutPath</key>
    <string>/Users/user/.memrec/memrecd.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/user/.memrec/memrecd.log</string>
    <key>WorkingDirectory</key>
    <string>/Users/user/.memrec</string>
</dict>
</plist>
```

### 步骤 5: 配置生成

创建 `~/.memrec/config.toml`:

```toml
version = "0.3.0"

[model]
model_type = "minilm-l6-v2"
source = "huggingface"
dimension = 384

[[model.files]]
filename = "model.onnx"
sha256 = "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"
required = true

# ... 更多带哈希的文件

[server]
socket_path = "~/.memrec/memrecd.sock"
data_dir = "~/.memrec/data"
vector_dir = "~/.memrec/vectors"
log_path = "~/.memrec/memrecd.log"
```

### 步骤 6: 验证测试

```bash
# 执行的测试：
1. ✅ 检查二进制文件是否可执行
2. ✅ 验证 socket 连接
3. ✅ 测试记忆添加
4. ✅ 测试记忆检索
5. ✅ 测试语义搜索
```

## 故障排除

### 常见问题

#### 安装失败

```bash
# 检查 Rust 安装
rustc --version
cargo --version

# 检查网络连接
curl -I https://crates.io
curl -I https://huggingface.co

# 尝试使用镜像
mr-install --use-hf-mirror

# 跳过验证（开发）
mr-install --skip-verify
```

#### 服务无法启动

```bash
# 检查日志
cat ~/.memrec/memrecd.log

# Linux: 检查 systemd 状态
systemctl --user status memrecd
journalctl --user -u memrecd

# macOS: 检查 launchd
launchctl list com.itcraft.memrecd
log stream --predicate 'subsystem == "com.itcraft.memrecd"'

# 手动启动
~/.local/bin/memrecd  # Linux
~/bin/memrecd         # macOS
```

#### 权限问题

```bash
# 检查目录权限
ls -la ~/.memrec/
ls -la ~/.local/bin/  # Linux
ls -la ~/bin/         # macOS

# 检查 socket 权限
ls -la ~/.memrec/memrecd.sock

# 修复权限（如果需要）
chmod 755 ~/.memrec
chmod 600 ~/.memrec/config.toml
```

### 调试模式

```bash
# 详细输出
RUST_LOG=debug mr-install

# 跟踪所有操作
RUST_LOG=trace mr-install
```

## 卸载

### 手动移除

```bash
# 停止并禁用服务
systemctl --user stop memrecd          # Linux
systemctl --user disable memrecd
launchctl stop com.itcraft.memrecd     # macOS
launchctl bootout gui/$UID ~/Library/LaunchAgents/com.itcraft.memrecd.plist

# 移除二进制文件
rm ~/.local/bin/memrec ~/.local/bin/memrecd ~/.local/bin/mr-install  # Linux
rm ~/bin/memrec ~/bin/memrecd ~/bin/mr-install                       # macOS

# 移除数据和配置
rm -rf ~/.memrec

# 移除服务文件
rm ~/.config/systemd/user/memrecd.service                            # Linux
rm ~/Library/LaunchAgents/com.itcraft.memrecd.plist                  # macOS
```

### 使用包管理器

```bash
# 通过 cargo 移除
cargo uninstall memrec memrecd memrec-common mr-install
```

## 开发

### 从源码构建

```bash
git clone https://github.com/itcraft-cn/memrec
cd memrec/mr-install

# 构建
cargo build --release

# 测试
cargo test --release

# 本地安装
cargo install --path .
```

### 添加新平台

要添加对新平台的支持，请实现 `ServiceManager` trait：

```rust
pub trait ServiceManager {
    fn name(&self) -> &str;
    fn register(&self, bin_path: &Path, home_dir: &Path) -> Result<()>;
    fn start(&self) -> Result<()>;
    fn stop(&self) -> Result<()>;
    fn is_active(&self) -> bool;
    fn unregister(&self) -> Result<()>;
}
```

## 安全考虑

### 风险评估

| 风险 | 缓解措施 | 默认 |
|------|------------|---------|
| 恶意模型注入 | SHA256 哈希验证 | 启用 |
| 不受信任的 Git 仓库 | 白名单强制执行 | 启用 |
| 服务命令注入 | 硬编码服务文件 | 启用 |
| 权限提升 | 用户模式服务 | 启用 |

### 推荐实践

1. **始终验证哈希**，除非绝对必要
2. **尽可能使用官方仓库**
3. **安装后检查服务文件**
4. **监控日志**中的异常活动
5. **定期更新**以获取安全修复

## 贡献

开发指南请参阅 [CONTRIBUTING.md](../CONTRIBUTING.md)。

## 许可证

Apache 许可证 2.0 - 详见 [LICENSE](../LICENSE)。

## 链接

- [主仓库](https://github.com/itcraft-cn/memrec)
- [Crates.io](https://crates.io/crates/mr-install)
- [CLI 客户端](../memrec/README.md)
- [守护进程服务器](../memrecd/README.md)
- [安全分析](../SECURITY_ANALYSIS.md)