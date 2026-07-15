use anyhow::Result;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use memrecd::daemon::Daemon;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("memrecd {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting memrecd v{}", env!("CARGO_PKG_VERSION"));

    let daemon = Daemon::new()?;

    tokio::runtime::Runtime::new()?.block_on(async { daemon.run().await })?;

    info!("memrecd stopped");
    Ok(())
}
