use anyhow::Result;
use clap::Parser;
use opencrab::{cli, logging};

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file before anything else (silently ignore if missing)
    dotenvy::dotenv().ok();

    // Parse CLI arguments first to check for debug flag
    let cli_args = cli::Cli::parse();

    // Initialize logging based on debug flag
    // - Debug mode OFF: No log files created, silent logging
    // - Debug mode ON: Creates log files in .opencrab/logs/, detailed logging
    let _guard = logging::setup_from_cli(cli_args.debug)
        .map_err(|e| anyhow::anyhow!("Failed to initialize logging: {}", e))?;

    // Clean up old log files (keep last 7 days)
    if cli_args.debug {
        if let Ok(removed) = logging::cleanup_old_logs(7) {
            if removed > 0 {
                tracing::info!("🧹 Cleaned up {} old log file(s)", removed);
            }
        }
    }

    // Run CLI application
    cli::run().await
}
