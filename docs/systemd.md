# Systemd Service 管理

## 快速安装

```bash
# 构建并安装二进制文件
cargo build --release
install -m 755 target/release/memrecd ~/.local/bin/
install -m 755 target/release/memrec ~/.local/bin/

# 安装 systemd 服务
./scripts/systemd/install.sh
```

## 基本使用

### systemctl --user 命令

```bash
# 启动守护进程
systemctl --user start memrecd

# 停止守护进程
systemctl --user stop memrecd

# 重启守护进程
systemctl --user restart memrecd

# 查看状态
systemctl --user status memrecd

# 查看日志
journalctl --user -u memrecd -f

# 启用开机自启（默认已启用）
systemctl --user enable memrecd

# 禁用开机自启
systemctl --user disable memrecd
```

### 快捷脚本

```bash
# 使用便捷管理脚本
./scripts/memrecctl.sh start     # 启动
./scripts/memrecctl.sh stop      # 停止
./scripts/memrecctl.sh restart   # 重启
./scripts/memrecctl.sh status    # 详细状态
./scripts/memrecctl.sh logs      # 查看日志
./scripts/memrecctl.sh install   # 安装服务
./scripts/memrecctl.sh uninstall # 卸载服务
```

## 服务配置

服务文件位置：`~/.config/systemd/user/memrecd.service`

**配置说明：**

```ini
[Service]
ExecStart=%h/.local/bin/memrecd        # 启动命令
Environment="RUST_LOG=info"             # 日志级别
Restart=on-failure                      # 失败自动重启
RestartSec=5                            # 重启间隔

# 日志输出
StandardOutput=append:%h/.memrec/memrecd.log
StandardError=append:%h/.memrec/memrecd.log
```

**修改配置：**

```bash
# 编辑服务文件
systemctl --user edit memrecd --full

# 重载配置
systemctl --user daemon-reload
systemctl --user restart memrecd
```

## 开机自启

systemd user service 在用户登录时自动启动。

**设置图形界面登录自动启动：**

```bash
# 启用 linger（即使未登录也能启动服务）
loginctl enable-linger $USER

# 启用服务
systemctl --user enable memrecd
```

## 日志查看

**journalctl 方式：**

```bash
# 查看所有日志
journalctl --user -u memrecd

# 实时跟踪日志
journalctl --user -u memrecd -f

# 查看最近100行
journalctl --user -u memrecd -n 100

# 查看今天的日志
journalctl --user -u memrecd --since today
```

**文件方式：**

```bash
# 日志文件位置
~/.memrec/memrecd.log

# 查看日志
tail -f ~/.memrec/memrecd.log
```

## 卸载

```bash
# 卸载服务（保留数据）
./scripts/systemd/uninstall.sh

# 手动卸载
systemctl --user stop memrecd
systemctl --user disable memrecd
rm ~/.config/systemd/user/memrecd.service
systemctl --user daemon-reload
```

## 常见问题

### Q: 服务无法启动

检查二进制文件路径：
```bash
ls ~/.local/bin/memrecd
```

查看错误日志：
```bash
journalctl --user -u memrecd -n 50 --no-pager
```

### Q: Socket 文件存在但无法连接

```bash
# 检查 socket 权限
ls -la ~/.memrec/memrecd.sock

# 重启服务
systemctl --user restart memrecd
```

### Q: 如何修改日志级别

```bash
# 编辑服务文件
systemctl --user edit memrecd --full

# 修改 Environment
Environment="RUST_LOG=debug"  # 更详细的日志

# 重载并重启
systemctl --user daemon-reload
systemctl --user restart memrecd
```

### Q: 如何限制资源使用

```bash
# 编辑服务文件添加限制
systemctl --user edit memrecd --full

# 添加资源限制
[Service]
MemoryMax=500M
CPUQuota=50%
```

## 文件位置

| 文件 | 位置 |
|------|------|
| 服务文件 | `~/.config/systemd/user/memrecd.service` |
| 二进制文件 | `~/.local/bin/memrecd` |
| Socket | `~/.memrec/memrecd.sock` |
| 数据目录 | `~/.memrec/data` |
| 日志文件 | `~/.memrec/memrecd.log` |
| 配置文件 | `~/.memrec/config.toml` (可选) |