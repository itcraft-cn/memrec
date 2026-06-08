use anyhow::Result;
use clap::Parser;

use mr_install::create_directories;
use mr_install::generate_config;
use mr_install::download_model;
use mr_install::DownloadOptions;
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
    step("Step 0/5: Pre-check");
    
    if which_exists("memrec") {
        ok("memrec found");
    } else {
        warn("memrec not found in PATH. Build and install memrec first.");
    }
    
    if which_exists("memrecd") {
        ok("memrecd found");
    } else {
        warn("memrecd not found in PATH. Build and install memrecd first.");
    }
    
    // Step 1: Create directories and generate config
    step("Step 1/5: Create directories and generate config");
    
    let home = create_directories()?;
    ok(&format!("Directories created: {}", home.display()));
    
    generate_config(&home)?;
    ok(&format!("Config generated: {}/config.toml", home.display()));
    
    // Step 2: Download model
    if cli.skip_model {
        step("Step 2/5: Model download (skipped)");
    } else {
        step("Step 2/5: Download embedding model (~90MB)");
        
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
                println!("    mr-install --skip-service --skip-verify");
                println!();
                println!("  Or set a custom model path:");
                println!("    export MEMREC_MODEL_DIR=/path/to/your/model");
                
                if !confirm("Continue without model?") {
                    std::process::exit(1);
                }
            }
        }
    }
    
    // Step 3: Register and start service
    if cli.skip_service {
        step("Step 3/5: Service registration (skipped)");
    } else {
        let svc = detect_service_manager();
        let svc_name = svc.name();
        step(&format!("Step 3/5: Register service ({})", svc_name));
        
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
    
    // Step 4: Verification
    if cli.skip_verify {
        step("Step 4/5: Verification (skipped)");
    } else {
        step("Step 4/5: Verify installation");
        
        match run_verification() {
            Ok(()) => ok("Verification passed"),
            Err(e) => warn(&format!("Verification failed: {}", e)),
        }
    }
    
    // Step 5: Summary
    step("Step 5/5: Installation complete");
    
    let home_str = home.display().to_string();
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║         MemRec installed successfully!              ║");
    println!("╠══════════════════════════════════════════════════════╣");
    println!("║                                                      ║");
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