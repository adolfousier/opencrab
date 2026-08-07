//! Install method detection.
//!
//! Determines how OpenCrabs was installed so that evolve, crash recovery,
//! and other update paths can use the correct upgrade strategy.

/// How the current binary was installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallMethod {
    /// Built from source — exe lives inside a cargo `target/` dir with a `Cargo.toml` ancestor.
    /// Contains the project root path.
    Source(std::path::PathBuf),
    /// Installed via `cargo install opencrabs` — exe lives in `~/.cargo/bin/`.
    CargoInstall,
    /// Installed by Homebrew — exe lives inside a Cellar under a brew prefix.
    ///
    /// Homebrew owns this file and records which version it believes is
    /// installed, so it has to do the upgrading. Renaming a downloaded binary
    /// over the Cellar copy succeeds (the prefix is user-owned on Apple
    /// Silicon) and leaves brew's manifest disagreeing with the disk, until an
    /// unrelated `brew upgrade` silently reverts the user (#963).
    Homebrew,
    /// Pre-built binary downloaded from GitHub releases (or installed manually).
    PrebuiltBinary,
}

impl InstallMethod {
    /// Detect how the current binary was installed.
    pub fn detect() -> Self {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return Self::PrebuiltBinary,
        };

        // Check if we're in a cargo target directory (source build)
        if let Some(project_root) = find_cargo_project(&exe) {
            return Self::Source(project_root);
        }

        // Check if we're in ~/.cargo/bin/ (cargo install)
        if is_in_cargo_bin(&exe) {
            return Self::CargoInstall;
        }

        // Before falling back: a brew-installed binary is a pre-built binary in
        // every respect except who owns it, so this must be tested first or it
        // is swallowed by the fallback below (#963).
        if is_in_homebrew_cellar(&exe) {
            return Self::Homebrew;
        }

        Self::PrebuiltBinary
    }

    /// Human-readable description for UI display.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Source(_) => "source build",
            Self::CargoInstall => "cargo install",
            Self::Homebrew => "homebrew",
            Self::PrebuiltBinary => "pre-built binary",
        }
    }
}

/// Is this executable inside a Homebrew Cellar?
///
/// Path-based on purpose. Asking `brew --prefix` needs brew on PATH, which it
/// often is not in a spawned process environment, and it costs a subprocess to
/// answer a question the path already answers. A Cellar layout is
/// `<prefix>/Cellar/<formula>/<version>/bin/<exe>`, and the two standard
/// prefixes are `/opt/homebrew` (Apple Silicon) and `/usr/local` (Intel and
/// Linuxbrew's default is `/home/linuxbrew/.linuxbrew`).
///
/// The `Cellar` component is what makes this safe: `/usr/local/bin` holds
/// plenty of hand-installed binaries that Homebrew does not own, and those must
/// keep being treated as loose binaries.
pub(crate) fn is_in_homebrew_cellar(exe: &std::path::Path) -> bool {
    const PREFIXES: [&str; 3] = ["/opt/homebrew", "/usr/local", "/home/linuxbrew/.linuxbrew"];
    let path = exe.to_string_lossy();
    PREFIXES.iter().any(|p| {
        path.starts_with(&format!("{p}/Cellar/"))
            // Homebrew also honours HOMEBREW_PREFIX, so accept any prefix whose
            // Cellar component is present rather than requiring a known root.
            || path.contains("/Cellar/opencrabs/")
    })
}

/// Walk up from the executable to find a Cargo.toml (indicating a source build).
fn find_cargo_project(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut dir = exe.parent()?.to_path_buf();
    loop {
        if dir.join("Cargo.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Check if the executable is in ~/.cargo/bin/.
fn is_in_cargo_bin(exe: &std::path::Path) -> bool {
    let cargo_bin = cargo_bin_dir();
    match cargo_bin {
        Some(dir) => exe.parent().map(|p| p == dir).unwrap_or(false),
        None => false,
    }
}

/// Get the cargo bin directory (~/.cargo/bin or $CARGO_HOME/bin).
fn cargo_bin_dir() -> Option<std::path::PathBuf> {
    if let Ok(cargo_home) = std::env::var("CARGO_HOME") {
        return Some(std::path::PathBuf::from(cargo_home).join("bin"));
    }
    dirs::home_dir().map(|h| h.join(".cargo").join("bin"))
}

/// Platform asset suffix for GitHub release downloads.
pub fn platform_suffix() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("macos-arm64"),
        ("macos", "x86_64") => Some("macos-amd64"),
        ("linux", "x86_64") => Some("linux-amd64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("windows", "x86_64") => Some("windows-amd64"),
        _ => None,
    }
}

/// Binary filename for the current platform.
pub fn binary_name() -> &'static str {
    if std::env::consts::OS == "windows" {
        "opencrabs.exe"
    } else {
        "opencrabs"
    }
}
