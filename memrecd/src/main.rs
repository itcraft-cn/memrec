use anyhow::Result;
use tracing_subscriber::FmtSubscriber;
use tracing::{info, Level};

use memrecd::daemon::Daemon;

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    
    tracing::subscriber::set_global_default(subscriber)?;
    
    info!("Starting memrecd v0.1.0");
    
    let daemon = Daemon::new()?;
    
    tokio::runtime::Runtime::new()?
        .block_on(async {
            daemon.run().await
        })?;
    
    info!("memrecd stopped");
    Ok(())
}