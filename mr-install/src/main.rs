use anyhow::Result;
use clap::Parser;

use mr_install::create_directories;
use mr_install::generate_config;
use mr_install::download_model;
use mr_install::DownloadOptions;
use mr_install::install_binaries;
use mr_install::InstallOptions;
use mr_install::detect_service_manager;
use mr_install::default_bin_dir;
use mr_install::run_verification;

#[derive(Parser)]
#[command(name = "mr-install")]
#[command(about = "Installer for MemRec — Local-first AI memory with project isolation", long_about = None)]
#[command(version)]
struct Cli {
    #[arg(long, help = "Use hf-mirror.com instead of huggingface.co")]
    use_hf_mirror: bool,
    
    #[arg(long, help = "Custom mirror base URL for model download")]
    mirror_base_url: Option<String>,
    
    #[arg(long, help = "Git repository URL for cargo install")]
    repo_url: Option<String>,
    
    #[arg(long, help = "Skip binary installation (cargo install)")]
    skip_install: bool,
    
    #[arg(long, help = "Skip model download")]
    skip_model: bool,
    
    #[arg(long, help = "Skip service registration")]
    skip_service: bool,
    
    #[arg(long, help = "Skip verification tests")]
    skip_verify: bool,
}

fn step(msg: &str) {
    println!("\n>>> {}\n", msg);
}

fn ok(msg: &str) {
    println!("[OK] {}", msg);
}

fn warn(msg: &str) {
    println!("[WARN] {}", msg);
}

fn fail(msg: &str) {
    println!("[FAIL] {}", msg);
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    println!("mr-install v{}", env!("CARGO_PKG_VERSION"));
    println!();
    
    // Step 0: Pre-check
    step("Step 0/6: Pre-check");
    
    if !which_exists("cargo") {
        fail("cargo not found. Install Rust first: https://rustup.rs");
        std::process::exit(1);
    }
    ok("cargo found");
    
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
    
    generate_config(&home)?;
    ok(&format!("Config generated: {}/config.toml", home.display()));
    
    // Step 3: Download model
    if cli.skip_model {
        step("Step 3/6: Model download (skipped)");
    } else {
        step("Step 3/6: Download embedding model (~90MB)");
        
        let opts = DownloadOptions {
            use_hf_mirror: cli.use_hf_mirror,
            mirror_base_url: cli.mirror_base_url.clone(),
        };
        
        match download_model(&opts).await {
            Ok(path) => ok(&format!("Model ready: {}", path.display())),
            Err(e) => {
                warn(&format!("Model download failed: {}", e));
                println!();
                println!("  You can manually download later:");
                println!("    mr-install --skip-install --skip-service --skip-verify");
                println!();
                println!("  Or set a custom model path:");
                println!("    export MEMREC_MODEL_DIR=/path/to/your/model");
                
                if !confirm("Continue without model?") {
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
    println!("║  Config:    {}/config.toml                      ║", home_str);
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

fn which_exists(cmd: &str) -> bool {
    #[cfg(target_family = "unix")]
    {
        std::process::Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    
    #[cfg(target_family = "windows")]
    {
        std::process::Command::new("where")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

fn confirm(msg: &str) -> bool {
    println!("{} (y/N) ", msg);
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    input.trim().eq_ignore_ascii_case("y")
}