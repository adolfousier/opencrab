//! Cross-platform PATH lookup shared by the CLI-backed providers.

/// Cross-platform binary lookup. Uses `where.exe` on Windows, `which` elsewhere.
/// Returns the resolved path if found on PATH, or None.
pub(crate) fn which_binary(name: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    let cmd = "where.exe";
    #[cfg(not(target_os = "windows"))]
    let cmd = "which";

    if let Ok(output) = std::process::Command::new(cmd).arg(name).output()
        && output.status.success()
    {
        // `where.exe` can return multiple lines — take the first
        let path = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }
    None
}
