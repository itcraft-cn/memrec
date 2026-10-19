//! # mr-install — MemRec 安装器
//!
//! 一键安装 MemRec 全套组件，六步流程：
//!
//! 1. 预检查（cargo 可用性、模型选择）
//! 2. 安装二进制（`cargo install`）
//! 3. 创建目录 + 生成配置
//! 4. 下载嵌入模型（HuggingFace/镜像）
//! 5. 注册并启动系统服务（systemd/launchd）
//! 6. 验证安装
//!
//! ## 包管理器安装场景（deb/rpm）
//!
//! 当二进制由包管理器安装时，可组合使用：
//!
//! - `--skip-install`：跳过 `cargo install`
//! - `--skip-cargo-check`：跳过 cargo 预检（无需 Rust 工具链）
//! - `--non-interactive`：无人值守（如 postinst），确认步骤默认否定
//!
//! 典型用法：`mr-install --skip-install`，由 mr-install 完成配置生成、
//! 模型下载与服务注册。

use anyhow::Result;
use clap::Parser;
use mr_common::ModelType;

use mr_install::create_directories;
use mr_install::default_bin_dir;
use mr_install::detect_service_manager;
use mr_install::download_model;
use mr_install::generate_config;
use mr_install::install_binaries;
use mr_install::run_verification;
use mr_install::DownloadOptions;
use mr_install::InstallOptions;

/// 安装器命令行参数
#[derive(Parser)]
#[command(name = "mr-install")]
#[command(about = "Installer for MemRec — Local-first AI memory with project isolation", long_about = None)]
#[command(version)]
struct Cli {
    /// 使用 hf-mirror.com 替代 huggingface.co
    #[arg(long, help = "Use hf-mirror.com instead of huggingface.co")]
    use_hf_mirror: bool,

    /// 自定义镜像基础 URL
    #[arg(long, help = "Custom mirror base URL for model download")]
    mirror_base_url: Option<String>,

    /// 自定义 Git 仓库 URL（用于 cargo install --git）
    #[arg(long, help = "Git repository URL for cargo install")]
    repo_url: Option<String>,

    /// 嵌入模型选择：bge-m3（默认，1024d，多语言/中文）或 minilm-l6-v2（384d，英文）
    #[arg(
        long,
        value_name = "MODEL",
        help = "Embedding model: bge-m3 (default, 1024d, multilingual/Chinese) or minilm-l6-v2 (384d, English)"
    )]
    model: Option<String>,

    /// 跳过二进制安装
    #[arg(long, help = "Skip binary installation (cargo install)")]
    skip_install: bool,

    /// 跳过模型下载
    #[arg(long, help = "Skip model download")]
    skip_model: bool,

    /// 跳过服务注册
    #[arg(long, help = "Skip service registration")]
    skip_service: bool,

    /// 跳过验证测试
    #[arg(long, help = "Skip verification tests")]
    skip_verify: bool,

    /// 跳过哈希校验（安全风险）
    #[arg(long, help = "Skip model hash verification (security risk)")]
    skip_hash_verify: bool,

    /// 允许任意 Git 仓库 URL（安全风险）
    #[arg(long, help = "Allow any Git repository URL (security risk)")]
    allow_any_repo: bool,

    /// 跳过 cargo 可用性预检。
    ///
    /// 适用于二进制已由包管理器（deb/rpm）安装的场景：此时无需 cargo，
    /// 通常与 `--skip-install` 配合使用。
    #[arg(
        long,
        help = "Skip cargo availability pre-check (for package-managed installs)"
    )]
    skip_cargo_check: bool,

    /// 非交互模式：需要确认的步骤一律取默认（否定）答案，不再读取标准输入。
    ///
    /// 适用于 postinst 脚本或 CI 等无人值守场景。
    #[arg(
        long,
        help = "Non-interactive mode: confirmation prompts default to no"
    )]
    non_interactive: bool,
}

/// 打印步骤标题
fn step(msg: &str) {
    println!("\n>>> {}\n", msg);
}

/// 打印成功消息
fn ok(msg: &str) {
    println!("[OK] {}", msg);
}

/// 打印警告消息
fn warn(msg: &str) {
    println!("[WARN] {}", msg);
}

/// 打印失败消息
fn fail(msg: &str) {
    println!("[FAIL] {}", msg);
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    println!("mr-install v{}", env!("CARGO_PKG_VERSION"));
    println!();

    let model_type = parse_model_type(cli.model.as_deref())?;
    let model_config = mr_common::ModelConfig::new(model_type.clone());

    if let Some(w) = model_type.warning() {
        warn(&w);
    }

    // Step 0: Pre-check
    step("Step 0/6: Pre-check");

    // 二进制安装（cargo install）需要 cargo；若已声明跳过安装或显式跳过预检，
    // 则不再要求 cargo，以满足包管理器安装场景（deb/rpm）。
    if cli.skip_install || cli.skip_cargo_check {
        if cli.skip_install {
            ok("cargo pre-check skipped (--skip-install)");
        } else {
            ok("cargo pre-check skipped (--skip-cargo-check)");
        }
    } else if !which_exists("cargo") {
        fail("cargo not found. Install Rust first: https://rustup.rs");
        std::process::exit(1);
    } else {
        ok("cargo found");
    }
    ok(&format!(
        "Model: {} ({}d)",
        model_type.name(),
        model_type.dimension()
    ));

    // Step 1: Install binaries
    if cli.skip_install {
        step("Step 1/6: Install binaries (skipped)");
        if !which_exists("memrec") || !which_exists("memrecd") {
            warn("memrec/memrecd not found. Install manually or remove --skip-install.");
        }
    } else {
        step("Step 1/6: Install binaries (cargo install)");

        let install_opts = InstallOptions {
            repo_url: cli.repo_url.clone(),
            allow_any_repo: cli.allow_any_repo,
        };

        match install_binaries(&install_opts) {
            Ok(bin_dir) => ok(&format!("Binaries installed to {}", bin_dir.display())),
            Err(e) => {
                fail(&format!("Binary installation failed: {}", e));
                std::process::exit(1);
            }
        }
    }

    // Step 2: Create directories and generate config
    step("Step 2/6: Create directories and generate config");

    let home = create_directories()?;
    ok(&format!("Directories created: {}", home.display()));

    generate_config(&home, &model_type)?;
    ok(&format!("Config generated: {}/config.toml", home.display()));

    // Step 3: Download model
    if cli.skip_model {
        step("Step 3/6: Model download (skipped)");
    } else {
        step("Step 3/6: Download embedding model");

        let opts = DownloadOptions {
            use_hf_mirror: cli.use_hf_mirror,
            mirror_base_url: cli.mirror_base_url.clone(),
            skip_hash_verify: cli.skip_hash_verify,
        };

        match download_model(&model_config, &opts).await {
            Ok(path) => ok(&format!("Model ready: {}", path.display())),
            Err(e) => {
                warn(&format!("Model download failed: {}", e));
                println!();
                println!("  You can manually download later:");
                println!(
                    "    mr-install --skip-install --skip-cargo-check --skip-service --skip-verify"
                );
                println!();
                println!("  Or set a custom model path:");
                println!("    export MEMREC_MODEL_DIR=/path/to/your/model");

                if !confirm("Continue without model?", cli.non_interactive) {
                    std::process::exit(1);
                }
            }
        }
    }

    // Step 4: Register and start service
    if cli.skip_service {
        step("Step 4/6: Service registration (skipped)");
    } else {
        let svc = detect_service_manager();
        let svc_name = svc.name();
        step(&format!("Step 4/6: Register service ({})", svc_name));

        let bin = default_bin_dir();

        match svc.register(&bin, &home) {
            Ok(()) => ok(&format!("Service registered ({})", svc_name)),
            Err(e) => {
                fail(&format!("Service registration failed: {}", e));
                std::process::exit(1);
            }
        }

        match svc.start() {
            Ok(()) => ok(&format!("Service started ({})", svc_name)),
            Err(e) => {
                fail(&format!("Service start failed: {}", e));
                std::process::exit(1);
            }
        }
    }

    // Step 5: Verification
    if cli.skip_verify {
        step("Step 5/6: Verification (skipped)");
    } else {
        step("Step 5/6: Verify installation");

        match run_verification() {
            Ok(()) => ok("Verification passed"),
            Err(e) => warn(&format!("Verification failed: {}", e)),
        }
    }

    // Step 6: Summary
    step("Step 6/6: Installation complete");

    let home_str = home.display().to_string();
    let bin_str = default_bin_dir().display().to_string();
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║         MemRec installed successfully!              ║");
    println!("╠══════════════════════════════════════════════════════╣");
    println!("║                                                      ║");
    println!("║  Binaries:  {}                     ║", bin_str);
    println!("║  Data:      {}/                             ║", home_str);
    println!(
        "║  Config:    {}/config.toml                      ║",
        home_str
    );
    println!("║  Socket:    {}/memrecd.sock                ║", home_str);
    println!("║                                                      ║");
    println!("╠══════════════════════════════════════════════════════╣");
    println!("║  Quick start:                                        ║");
    println!("║    memrec add \"hello\" --mtype knowledge             ║");
    println!("║    memrec search \"hello\"                            ║");
    println!("║    memrec stats                                      ║");
    println!("║                                                      ║");
    println!("╚══════════════════════════════════════════════════════╝");

    Ok(())
}

/// 检查命令是否存在于 PATH 中
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 交互式确认提示，默认为否。
///
/// 非交互模式（`non_interactive = true`）下不读取标准输入，直接返回否定答案，
/// 供 postinst/CI 等无人值守场景使用。
fn confirm(msg: &str, non_interactive: bool) -> bool {
    if non_interactive {
        println!("{} [non-interactive: no]", msg);
        return false;
    }

    println!("{} (y/N) ", msg);
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    input.trim().eq_ignore_ascii_case("y")
}

/// 解析模型类型字符串，支持 bge-m3 和 minilm-l6-v2
fn parse_model_type(model: Option<&str>) -> Result<ModelType> {
    match model {
        None | Some("bge-m3") => Ok(ModelType::BGEM3),
        Some("minilm-l6-v2") => Ok(ModelType::MiniLML6V2),
        Some(other) => anyhow::bail!("Unknown model '{}'. Supported: bge-m3, minilm-l6-v2", other),
    }
}
