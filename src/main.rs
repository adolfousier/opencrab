//! OpenCrabs binary entry point.
//!
//! See the [`opencrabs`] library crate for full documentation.

use anyhow::Result;
use clap::Parser;
use opencrabs::{cli, logging};

#[tokio::main]
async fn main() -> Result<()> {
    // Install rustls crypto provider before any TLS connections (Slack Socket Mode)
    #[cfg(feature = "slack")]
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Parse CLI arguments first to check for debug flag
    let cli_args = cli::Cli::parse();

    // Initialize logging based on --debug flag
    let mut log_config = logging::LogConfig::new().with_debug_mode(cli_args.debug);

    // Custom log directory from env
    if let Ok(log_dir) = std::env::var("DEBUG_LOGS_LOCATION") {
        log_config = log_config.with_log_dir(std::path::PathBuf::from(log_dir));
    }

    let _guard = logging::init_logging(log_config)
        .map_err(|e| anyhow::anyhow!("Failed to initialize logging: {}", e))?;

    // Clean up old log files (keep last 7 days)
    if cli_args.debug
        && let Ok(removed) = logging::cleanup_old_logs(7)
        && removed > 0
    {
        tracing::info!("🧹 Cleaned up {} old log file(s)", removed);
    }

    // Clean up orphaned channel temp files (tg_photo_*, wa_img_*) older than 3 days
    if let Ok(removed) = logging::cleanup_old_temp_files(3)
        && removed > 0
    {
        tracing::info!("🧹 Cleaned up {} orphaned temp file(s)", removed);
    }

    // Run CLI application
    let result = cli::run().await;

    // Print the error chain to stderr on failure. Without this, a CLI
    // subcommand that returns Err (e.g. `opencrabs cron list` hitting a
    // bad row) exits with status 1 and zero output, which makes
    // diagnosis a guessing game. The TUI's own error surfacing
    // (`tracing` + on-screen alerts) handles its lifecycle separately,
    // so this only fires for non-TUI CLI subcommands.
    if let Err(ref e) = result {
        eprintln!("Error: {e:#}");
    }

    // Use libc::_exit instead of std::process::exit — skips C atexit handlers
    // which avoids llama.cpp Metal device destructor crash on macOS ARM.
    // Still force-exits so background tokio tasks (embedding backfill) don't hang.
    let code = if result.is_ok() { 0 } else { 1 };
    unsafe { libc::_exit(code) }
}
