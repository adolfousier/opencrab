//! OpenCrabs binary entry point.
//!
//! See the [`opencrabs`] library crate for full documentation.

use anyhow::Result;
use clap::Parser;
use opencrabs::{cli, logging};

fn main() -> Result<()> {
    // Build the runtime by hand instead of `#[tokio::main]` so we can enlarge
    // the worker stack. OpenCrabs' agent turn (tool loop -> provider stream ->
    // tool dispatch) and the RSI digest are very deeply nested async state
    // machines; on the default 2MB worker stack a single poll can overflow and
    // abort the whole process with "thread 'tokio-rt-worker' has overflowed its
    // stack". 16MB gives that nesting ample headroom.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build tokio runtime: {}", e))?;
    runtime.block_on(async_main())
}

async fn async_main() -> Result<()> {
    // Install rustls crypto provider before any TLS connections (Slack Socket Mode)
    #[cfg(feature = "slack")]
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Parse CLI arguments first to check for debug flag
    let cli_args = cli::Cli::parse();

    // Initialize logging based on --debug flag. Resolve the log directory via
    // logging::log_dir() (DEBUG_LOGS_LOCATION override, else ~/.opencrabs/logs)
    // so the writer and the `logs status`/`logs view` readers always agree.
    let log_config = logging::LogConfig::new()
        .with_debug_mode(cli_args.debug)
        .with_log_dir(logging::log_dir());

    let _guard = logging::init_logging(log_config)
        .map_err(|e| anyhow::anyhow!("Failed to initialize logging: {}", e))?;

    // Clean up old log files (keep last 7 days)
    if cli_args.debug
        && let Ok(removed) = logging::cleanup_old_logs(7)
        && removed > 0
    {
        tracing::info!("🧹 Cleaned up {} old log file(s)", removed);
    }

    // Clean up orphaned channel temp files (tg_photo_*, wa_img_*) older than 30 days.
    // Never wipe on restart — files may be recoverable or still needed.
    if let Ok(removed) = logging::cleanup_old_temp_files(30)
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
