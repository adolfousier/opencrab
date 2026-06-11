//! Logging and Debug System
//!
//! Provides configurable logging with conditional file output for debug mode.

use std::path::PathBuf;
use tracing::Level;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Local-time formatter using chrono — matches the system timezone.
struct LocalTime;

impl FormatTime for LocalTime {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let now = chrono::Local::now();
        write!(w, "{}", now.format("%Y-%m-%dT%H:%M:%S%.6f%:z"))
    }
}

/// Filename prefix for rolling daily log files. The rolling appender writes
/// `<prefix>.YYYY-MM-DD` (e.g. `opencrabs.2026-06-10`) — NO `.log` extension.
/// Single source of truth shared by the writer config and the readers
/// (`logs status` / `logs view` / cleanup), so they can't drift on the name.
pub const DEFAULT_LOG_PREFIX: &str = "opencrabs";

/// Logging configuration
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Enable debug mode (creates log files)
    pub debug_mode: bool,

    /// Log directory path (default: .opencrabs/logs)
    pub log_dir: PathBuf,

    /// Minimum log level (default: INFO, DEBUG mode: DEBUG)
    pub log_level: Level,

    /// Enable console output (for non-TUI modes)
    pub console_output: bool,

    /// Log file name prefix
    pub log_prefix: String,

    /// Maximum log file age in days (for rotation)
    pub max_age_days: u64,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            debug_mode: false,
            log_dir: crate::config::opencrabs_home().join("logs"),
            log_level: Level::INFO,
            console_output: false,
            log_prefix: DEFAULT_LOG_PREFIX.to_string(),
            max_age_days: 7,
        }
    }
}

impl LogConfig {
    /// Create a new log configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable debug mode (creates log files with DEBUG level)
    pub fn with_debug_mode(mut self, enabled: bool) -> Self {
        self.debug_mode = enabled;
        if enabled {
            self.log_level = Level::DEBUG;
        }
        self
    }

    /// Set custom log directory
    pub fn with_log_dir(mut self, dir: PathBuf) -> Self {
        self.log_dir = dir;
        self
    }

    /// Set log level
    pub fn with_log_level(mut self, level: Level) -> Self {
        self.log_level = level;
        self
    }

    /// Enable console output
    pub fn with_console_output(mut self, enabled: bool) -> Self {
        self.console_output = enabled;
        self
    }

    /// Set log file prefix
    pub fn with_log_prefix(mut self, prefix: String) -> Self {
        self.log_prefix = prefix;
        self
    }
}

/// Result of logger initialization. Held by `main` for the whole program.
///
/// File logging is synchronous (no `tracing_appender::non_blocking` worker), so
/// there is no background flush-on-drop guard to keep alive — every event is
/// written on the calling thread. This stays as a marker type so the
/// `init_logging` API and `main`'s `let _guard = …` contract are unchanged.
pub struct LoggerGuard;

impl LoggerGuard {
    fn empty() -> Self {
        Self
    }
}

/// A synchronous, self-healing daily rolling file writer for tracing (#190).
///
/// Wraps `tracing_appender::rolling::daily` (reused for correct UTC date
/// handling and rotation) but deliberately avoids `tracing_appender::non_blocking`:
///
///   * The non-blocking worker thread swallows IO errors (`worker.rs`:
///     `Err(_) => {}`) while still draining the channel, so after a single
///     write failure every later line is dropped silently and the file freezes
///     with the process alive and no error surfaced. It also drops events when
///     its bounded buffer fills under load.
///   * Writing on the calling thread surfaces write errors to
///     tracing-subscriber's stderr fallback instead of vanishing.
///   * On a write error the inner appender is rebuilt, so the next event
///     reopens the file — recovering from an fd closed out-of-band (logrotate,
///     external close) instead of staying frozen until restart. The rolling
///     appender alone only reopens on date rollover.
///
/// Debug file logging is opt-in (`-d`), so the synchronous IO cost is an
/// acceptable trade for a log that is actually reliable.
pub(crate) struct ResilientFileWriter {
    log_dir: PathBuf,
    prefix: String,
    appender: std::sync::Mutex<tracing_appender::rolling::RollingFileAppender>,
}

impl ResilientFileWriter {
    pub(crate) fn new(log_dir: PathBuf, prefix: String) -> Self {
        let appender = tracing_appender::rolling::daily(&log_dir, &prefix);
        Self {
            log_dir,
            prefix,
            appender: std::sync::Mutex::new(appender),
        }
    }
}

impl<'a> tracing_subscriber::fmt::writer::MakeWriter<'a> for ResilientFileWriter {
    type Writer = ResilientFileGuard<'a>;
    fn make_writer(&'a self) -> Self::Writer {
        ResilientFileGuard {
            parent: self,
            appender: self.appender.lock().unwrap_or_else(|e| e.into_inner()),
        }
    }
}

pub(crate) struct ResilientFileGuard<'a> {
    parent: &'a ResilientFileWriter,
    appender: std::sync::MutexGuard<'a, tracing_appender::rolling::RollingFileAppender>,
}

impl std::io::Write for ResilientFileGuard<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let result = self.appender.write(buf);
        if result.is_err() {
            // Self-heal: rebuild the appender so the next event reopens the file
            // instead of every subsequent write hitting the same dead handle.
            *self.appender =
                tracing_appender::rolling::daily(&self.parent.log_dir, &self.parent.prefix);
        }
        result
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.appender.flush()
    }
}

/// Initialize the logging system
///
/// Returns a guard that must be kept alive for the duration of the program.
/// When the guard is dropped, logs are flushed.
///
/// # Arguments
/// * `config` - Logging configuration
///
/// # Behavior
/// - **Debug mode OFF**: No log files created, minimal console output
/// - **Debug mode ON**: Creates log files in `.opencrabs/logs/`, detailed logging
pub fn init_logging(config: LogConfig) -> Result<LoggerGuard, Box<dyn std::error::Error>> {
    if config.debug_mode {
        // Debug mode: Create log files in .opencrabs/logs/
        init_debug_logging(config)
    } else {
        // Normal mode: Minimal logging, no files
        init_minimal_logging(config)
    }
}

/// Initialize debug logging with file output
fn init_debug_logging(config: LogConfig) -> Result<LoggerGuard, Box<dyn std::error::Error>> {
    // Create log directory
    std::fs::create_dir_all(&config.log_dir)?;

    // Create gitignore file in .opencrabs to ignore logs
    let opencrabs_dir = config.log_dir.parent().unwrap_or(&config.log_dir);
    let gitignore_path = opencrabs_dir.join(".gitignore");
    if !gitignore_path.exists() {
        std::fs::write(
            &gitignore_path,
            "# Ignore all OpenCrabs runtime files\n*\n!.gitignore\n",
        )
        .ok();
    }

    // Set up the daily rolling file writer, written SYNCHRONOUSLY and
    // self-healing — see `ResilientFileWriter` for the full rationale (#190).
    // In short: no `non_blocking` worker thread to silently die and drop every
    // event, and a write failure rebuilds the appender so the next event
    // reopens the file instead of the log freezing forever.
    let file_appender = ResilientFileWriter::new(config.log_dir.clone(), config.log_prefix.clone());

    // Build environment filter
    let env_filter = EnvFilter::from_default_env()
        .add_directive(config.log_level.into())
        .add_directive("rusqlite=warn".parse()?)
        .add_directive("hyper=warn".parse()?)
        .add_directive("h2=warn".parse()?)
        .add_directive("reqwest=warn".parse()?)
        .add_directive("tower=warn".parse()?)
        .add_directive("slack_morphism=warn".parse()?)
        // whatsapp-rust logs TODO stubs for unimplemented upstream handlers — suppress
        .add_directive("whatsapp_rust::client=error".parse()?)
        .add_directive("whatsapp_rust=warn".parse()?);

    // Initialize subscriber with file logging
    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_appender)
                .with_timer(LocalTime)
                .with_ansi(false) // No colors in log files
                .with_target(true)
                .with_thread_ids(true)
                .with_line_number(true)
                .with_file(true),
        )
        .init();

    // Log startup information
    tracing::info!("🚀 OpenCrabs debug mode enabled");
    tracing::info!("📁 Log directory: {}", config.log_dir.display());
    tracing::info!("📊 Log level: {:?}", config.log_level);
    tracing::debug!("Debug logging initialized successfully");

    Ok(LoggerGuard::empty())
}

/// Initialize minimal logging (no file output)
fn init_minimal_logging(config: LogConfig) -> Result<LoggerGuard, Box<dyn std::error::Error>> {
    // Build environment filter - minimal logging
    let env_filter = EnvFilter::from_default_env()
        .add_directive(Level::WARN.into()) // Only warnings and errors
        .add_directive("opencrabs=info".parse()?); // INFO for opencrabs itself

    if config.console_output {
        // Console output for non-TUI modes
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_timer(LocalTime)
                    .with_ansi(true)
                    .with_target(false)
                    .compact(),
            )
            .init();
    } else {
        // Silent mode for TUI (no output to avoid interference)
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::sink))
            .init();
    }

    Ok(LoggerGuard::empty())
}

/// Convenience function to setup logging from CLI args
pub fn setup_from_cli(debug: bool) -> Result<LoggerGuard, Box<dyn std::error::Error>> {
    let config = LogConfig::new().with_debug_mode(debug);
    init_logging(config)
}

/// Resolve the directory where debug log files live — the same path the writer
/// uses (`DEBUG_LOGS_LOCATION` override, else `~/.opencrabs/logs`). Readers MUST
/// resolve this rather than a CWD-relative path, or a daemon (whose working dir
/// isn't home) reports an empty directory (#190 secondary).
pub fn log_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("DEBUG_LOGS_LOCATION") {
        PathBuf::from(dir)
    } else {
        crate::config::opencrabs_home().join("logs")
    }
}

/// Whether a directory entry filename is one of our rolling daily log files
/// (`<prefix>.YYYY-MM-DD`). The files carry NO `.log` extension, so the old
/// `extension == "log"` checks matched zero files — making `logs status` report
/// 0, `logs view` find nothing, and `cleanup_old_logs` never prune (#190).
pub fn is_log_file(file_name: &str) -> bool {
    file_name
        .strip_prefix(DEFAULT_LOG_PREFIX)
        .is_some_and(|rest| rest.starts_with('.'))
}

/// Get the path to the current (most recent) log file, if any exist.
pub fn get_log_path() -> Option<PathBuf> {
    let dir = log_dir();
    if !dir.exists() {
        return None;
    }
    std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_str().is_some_and(is_log_file))
        .max_by_key(|entry| entry.metadata().ok()?.modified().ok())
        .map(|entry| entry.path())
}

/// Clean up old log files based on max age
pub fn cleanup_old_logs(max_age_days: u64) -> Result<usize, Box<dyn std::error::Error>> {
    let dir = log_dir();
    if !dir.exists() {
        return Ok(0);
    }

    let max_age = std::time::Duration::from_secs(max_age_days * 24 * 60 * 60);
    let now = std::time::SystemTime::now();
    let mut removed = 0;

    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();

        if entry.file_name().to_str().is_some_and(is_log_file)
            && let Ok(metadata) = entry.metadata()
            && let Ok(modified) = metadata.modified()
            && let Ok(age) = now.duration_since(modified)
            && age > max_age
            && std::fs::remove_file(&path).is_ok()
        {
            removed += 1;
        }
    }

    Ok(removed)
}

/// Clean up orphaned temp files from ~/.opencrabs/tmp/files/ older than max_age_days.
/// All channel image uploads (Telegram, WhatsApp, Slack, Trello) are saved here
/// via process_file_with_vision. This single purge replaces per-channel cleanup spawns.
pub fn cleanup_old_temp_files(max_age_days: u64) -> Result<usize, Box<dyn std::error::Error>> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Ok(0),
    };
    let tmp_dir = home.join(".opencrabs").join("tmp").join("files");
    if !tmp_dir.exists() {
        return Ok(0);
    }

    let max_age = std::time::Duration::from_secs(max_age_days * 24 * 60 * 60);
    let now = std::time::SystemTime::now();
    let mut removed = 0;

    for entry in std::fs::read_dir(&tmp_dir)? {
        let entry = entry?;
        let path = entry.path();

        // Clean all files in our temp directory (images, PDFs, etc.)
        if !path.is_file() {
            continue;
        }

        if let Ok(metadata) = entry.metadata()
            && let Ok(modified) = metadata.modified()
            && let Ok(age) = now.duration_since(modified)
            && age > max_age
            && std::fs::remove_file(&path).is_ok()
        {
            removed += 1;
        }
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_config_default() {
        let config = LogConfig::default();
        assert!(!config.debug_mode);
        assert_eq!(config.log_level, Level::INFO);
        assert!(!config.console_output);
        assert_eq!(config.log_prefix, "opencrabs");
    }

    #[test]
    fn test_log_config_with_debug() {
        let config = LogConfig::new().with_debug_mode(true);
        assert!(config.debug_mode);
        assert_eq!(config.log_level, Level::DEBUG);
    }

    #[test]
    fn test_log_config_builder() {
        let config = LogConfig::new()
            .with_log_level(Level::TRACE)
            .with_console_output(true)
            .with_log_prefix("test".to_string());

        assert_eq!(config.log_level, Level::TRACE);
        assert!(config.console_output);
        assert_eq!(config.log_prefix, "test");
    }

    #[test]
    fn test_log_dir_in_home_opencrabs_folder() {
        let config = LogConfig::default();
        let log_dir_str = config.log_dir.to_string_lossy();
        assert!(log_dir_str.contains(".opencrabs"));
        assert!(log_dir_str.contains("logs"));
        // Should be under home dir, not cwd
        if let Some(home) = dirs::home_dir() {
            assert!(config.log_dir.starts_with(&home));
        }
    }
}
