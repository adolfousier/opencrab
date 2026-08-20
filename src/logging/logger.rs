//! Logging and Debug System
//!
//! Provides configurable logging with conditional file output for debug mode.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::Level;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

/// Runtime gate for debug file logging (#678). The file layer is always
/// attached to the subscriber but only writes while this is `true`; the
/// per-event filter (`debug_logs_enabled`) reads it, so flipping it turns file
/// logging on/off live without re-initializing the subscriber (which tracing
/// forbids). Seeded from the launch-time `--debug` state and re-applied on
/// every config hot-reload.
static DEBUG_LOGS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether `--debug`/`-d` was passed on the command line. The flag always wins:
/// a config edit that sets `debug_logs = false` must never silence an operator
/// who explicitly launched with `-d`. Effective state is `forced || config`.
static DEBUG_FORCED_BY_FLAG: AtomicBool = AtomicBool::new(false);

/// Resolve the effective debug-logging state: the `--debug` flag OR the config
/// `agent.debug_logs` toggle. Pure so it can be unit-tested without touching the
/// process-global subscriber state.
pub fn effective_debug_logs(forced_by_flag: bool, config_enabled: bool) -> bool {
    forced_by_flag || config_enabled
}

/// Whether debug file logging is currently on. Read per-event by the file
/// layer's gate filter.
pub fn debug_logs_enabled() -> bool {
    DEBUG_LOGS_ENABLED.load(Ordering::Relaxed)
}

/// Flip the runtime gate, logging the transition only when it actually changes
/// so a no-op reload stays quiet.
fn set_debug_logs(enabled: bool) {
    let previous = DEBUG_LOGS_ENABLED.swap(enabled, Ordering::Relaxed);
    if previous != enabled {
        tracing::info!(
            "🔧 debug_logs {} via config",
            if enabled { "ENABLED" } else { "DISABLED" }
        );
    }
}

/// Apply the effective debug-logging state from the launch-time flag and the
/// config toggle, recording the flag so later hot-reloads keep honoring it.
/// Called once at startup after config loads.
pub fn apply_debug_logs(forced_by_flag: bool, config_enabled: bool) {
    DEBUG_FORCED_BY_FLAG.store(forced_by_flag, Ordering::Relaxed);
    set_debug_logs(effective_debug_logs(forced_by_flag, config_enabled));
}

/// Re-apply the effective state from a hot-reloaded config, preserving the
/// launch-time `--debug` flag. Called from the config-watcher callback.
pub fn apply_debug_logs_from_config(config_enabled: bool) {
    let forced = DEBUG_FORCED_BY_FLAG.load(Ordering::Relaxed);
    set_debug_logs(effective_debug_logs(forced, config_enabled));
}

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
    // Built lazily on the first actual write, NOT at construction. The file
    // layer is always attached but runtime-gated by `debug_logs_enabled()`
    // (#678): when the gate is off, no event reaches `make_writer`, so a
    // disabled process never creates an empty log directory or file. The
    // appender is only materialized once debug logging is genuinely turned on.
    appender: std::sync::Mutex<Option<tracing_appender::rolling::RollingFileAppender>>,
}

impl ResilientFileWriter {
    pub(crate) fn new(log_dir: PathBuf, prefix: String) -> Self {
        Self {
            log_dir,
            prefix,
            appender: std::sync::Mutex::new(None),
        }
    }

    /// Test-only: create a writer with a temporary directory (#1077).
    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        let temp_dir =
            std::env::temp_dir().join(format!("opencrabs_log_test_{}", std::process::id()));
        Self::new(temp_dir, "test".to_string())
    }

    /// Test-only: acquire the inner mutex to simulate a blocked write (#1077).
    #[cfg(test)]
    pub(crate) fn appender_lock_for_test(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, Option<tracing_appender::rolling::RollingFileAppender>>,
        std::sync::TryLockError<()>,
    > {
        self.appender
            .lock()
            .map_err(|_| std::sync::TryLockError::WouldBlock)
    }

    /// Create the log directory (+ a `.gitignore` covering all runtime files)
    /// and open the daily rolling appender. Called on the first write and on
    /// self-heal after a write failure.
    fn build(log_dir: &PathBuf, prefix: &str) -> tracing_appender::rolling::RollingFileAppender {
        // Best-effort dir + gitignore. Failures here surface as write errors
        // below (which self-heal), so ignore the Result rather than panic.
        if std::fs::create_dir_all(log_dir).is_ok() {
            let gitignore_path = log_dir
                .parent()
                .unwrap_or(log_dir.as_path())
                .join(".gitignore");
            if !gitignore_path.exists() {
                std::fs::write(
                    &gitignore_path,
                    "# Ignore all OpenCrabs runtime files\n*\n!.gitignore\n",
                )
                .ok();
            }
        }
        tracing_appender::rolling::daily(log_dir, prefix)
    }
}

impl<'a> tracing_subscriber::fmt::writer::MakeWriter<'a> for ResilientFileWriter {
    type Writer = ResilientFileGuard<'a>;
    fn make_writer(&'a self) -> Self::Writer {
        // Block on the lock rather than skipping the event.
        //
        // This used `try_lock` and dropped the event on `WouldBlock`, on the
        // reasoning that a held lock meant a thread stuck on a slow write.
        // `WouldBlock` only means some other thread holds it right now, which
        // in an agent logging from tool execution, streaming, channels and the
        // TUI at once is the normal case. The guard fired constantly and threw
        // log lines away under ordinary load (#1115).
        //
        // Appender writes are fast and every other consumer of this mutex
        // already blocks on it, so blocking here is both correct and what the
        // rest of the code assumes.
        //
        // Poisoning recovery below is the part that fixes #1077: a panic while
        // holding the lock must not silence logging for the rest of the run.
        let appender = match self.appender.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        ResilientFileGuard {
            parent: self,
            appender: Some(appender),
        }
    }
}

pub(crate) struct ResilientFileGuard<'a> {
    parent: &'a ResilientFileWriter,
    appender:
        Option<std::sync::MutexGuard<'a, Option<tracing_appender::rolling::RollingFileAppender>>>,
}

impl std::io::Write for ResilientFileGuard<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // If the guard is None (mutex contention), discard the write (#1077).
        let guard = match self.appender.as_mut() {
            Some(g) => g,
            None => return Ok(buf.len()), // pretend success to avoid error spam
        };
        // Lazily open the appender on the first write (see `ResilientFileWriter`).
        if guard.is_none() {
            **guard = Some(ResilientFileWriter::build(
                &self.parent.log_dir,
                &self.parent.prefix,
            ));
        }
        let appender = guard.as_mut().expect("appender was just materialized");
        let result = appender.write(buf);
        if result.is_err() {
            // Self-heal: rebuild the appender so the next event reopens the file
            // instead of every subsequent write hitting the same dead handle.
            **guard = Some(ResilientFileWriter::build(
                &self.parent.log_dir,
                &self.parent.prefix,
            ));
        }
        result
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.appender.as_mut() {
            Some(guard) => match guard.as_mut() {
                Some(appender) => appender.flush(),
                None => Ok(()),
            },
            None => Ok(()),
        }
    }
}

/// Initialize the logging system.
///
/// Returns a guard that must be kept alive for the duration of the program.
///
/// # Behavior (#678)
/// A single subscriber is installed once (tracing forbids re-init), carrying two
/// layers whose on/off state is controlled at runtime rather than baked in:
/// - **File layer** — daily rolling files in `~/.opencrabs/logs/`, gated per
///   event by [`debug_logs_enabled`]. The gate is seeded here from
///   `config.debug_mode` (the effective `--debug` state) and can later be
///   flipped by config hot-reload via [`apply_debug_logs_from_config`]. While
///   the gate is off, no event reaches the (lazy) writer, so no log files are
///   created — matching the old "no `-d`, no files" behavior.
/// - **Console layer** — minimal warnings, to stderr for non-TUI callers or to
///   `sink` for the TUI (avoids corrupting the terminal UI).
pub fn init_logging(config: LogConfig) -> Result<LoggerGuard, Box<dyn std::error::Error>> {
    use tracing_subscriber::filter::filter_fn;
    use tracing_subscriber::fmt::writer::BoxMakeWriter;

    // Seed the runtime gate + the forced-by-flag marker from the launch-time
    // debug mode. run() re-applies the config toggle on top once config loads.
    DEBUG_LOGS_ENABLED.store(config.debug_mode, Ordering::Relaxed);
    DEBUG_FORCED_BY_FLAG.store(config.debug_mode, Ordering::Relaxed);

    // File layer: DEBUG detail plus the third-party noise directives. Level is
    // fixed at DEBUG (not `config.log_level`) so the file captures full detail
    // whenever the gate is ON, including when a hot-reload enables it from a
    // process that launched without `-d`.
    let file_env = EnvFilter::from_default_env()
        .add_directive(Level::DEBUG.into())
        .add_directive("rusqlite=warn".parse()?)
        .add_directive("hyper=warn".parse()?)
        .add_directive("h2=warn".parse()?)
        .add_directive("reqwest=warn".parse()?)
        .add_directive("tower=warn".parse()?)
        .add_directive("slack_morphism=warn".parse()?)
        // whatsapp-rust logs TODO stubs for unimplemented upstream handlers — suppress
        .add_directive("whatsapp_rust::client=error".parse()?)
        .add_directive("whatsapp_rust=warn".parse()?)
        // whatsapp-rust also emits under CUSTOM targets that the module
        // directives above never match: "Client/Keepalive" pings every ~30s
        // and "UnifiedSession" time-offset/append chatter (thousands of
        // lines a day between them, #400).
        .add_directive("Client=warn".parse()?)
        .add_directive("UnifiedSession=warn".parse()?);

    // Self-healing synchronous daily rolling writer — see `ResilientFileWriter`
    // for the #190 rationale; now lazy so a disabled gate creates no files.
    let file_appender = ResilientFileWriter::new(config.log_dir.clone(), config.log_prefix.clone());
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_appender)
        .with_timer(LocalTime)
        .with_ansi(false) // No colors in log files
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_file(true)
        .with_filter(file_env)
        // Runtime on/off gate. Re-evaluated per event so config hot-reload takes
        // effect immediately; keep it as the outer filter so `make_writer` (and
        // thus lazy file creation) never fires while debug logging is off.
        .with_filter(filter_fn(|_meta| debug_logs_enabled()));

    // Console layer: minimal, WARN + opencrabs=info. stderr for non-TUI, sink
    // for the TUI so log lines never corrupt the on-screen interface.
    let console_env = EnvFilter::from_default_env()
        .add_directive(Level::WARN.into())
        .add_directive("opencrabs=info".parse()?);
    let console_writer: BoxMakeWriter = if config.console_output {
        BoxMakeWriter::new(std::io::stderr)
    } else {
        BoxMakeWriter::new(std::io::sink)
    };
    let console_layer = tracing_subscriber::fmt::layer()
        .with_writer(console_writer)
        .with_timer(LocalTime)
        .with_ansi(config.console_output)
        .with_target(false)
        .compact()
        .with_filter(console_env);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .init();

    if debug_logs_enabled() {
        tracing::info!("🚀 OpenCrabs debug logging enabled");
        tracing::info!("📁 Log directory: {}", config.log_dir.display());
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
    // Profile-aware: purge the active profile's tmp/files (where save_to_temp
    // writes), not the default root — otherwise a profile's temp files never
    // get cleaned.
    let tmp_dir = crate::config::opencrabs_home().join("tmp").join("files");
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
