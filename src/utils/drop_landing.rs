//! Where a file pulled over the drop tunnel lands on this machine (#1311).
//!
//! The copy keeps the client's own filename so `<home>/tmp/` stays readable:
//! a drop called `Screenshot 2026-09-02.png` is a file called
//! `Screenshot 2026-09-02.png` here too, not `dropped-1725300000000.png`. A
//! timestamp is spliced in only when that name is already taken, because two
//! screenshots called `Screenshot.png` must not clobber each other.

use std::path::{Path, PathBuf};

/// The filename the client dropped, as it will be reused on this side.
///
/// Splits on both separators because the client may be Windows while this
/// side is not, in which case `Path::file_name` would keep the whole
/// `C:\Users\...` string as the "name".
pub fn client_file_name(client_path: &str) -> String {
    let name = client_path
        .trim()
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim();
    if name.is_empty() || name == "." || name == ".." {
        "dropped-file".to_string()
    } else {
        name.to_string()
    }
}

/// Where the pulled bytes go inside `dir`: the client's filename, or on a
/// collision `<stem>-<stamp>.<ext>`.
///
/// `taken` is injected rather than read from the disk so the choice is a pure
/// function and every branch is testable.
pub fn landing_path(
    dir: &Path,
    client_path: &str,
    stamp: u128,
    taken: impl Fn(&Path) -> bool,
) -> PathBuf {
    let name = client_file_name(client_path);
    let plain = dir.join(&name);
    if !taken(&plain) {
        return plain;
    }
    let as_path = Path::new(&name);
    let stem = as_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| name.clone());
    let ext = as_path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    dir.join(format!("{stem}-{stamp}{ext}"))
}

/// The one-line receipt shown in the TUI after a pull, so the user can see
/// that the file was copied to this machine and where it now lives.
pub fn pulled_notice(name: &str, landed: &str, bytes: usize) -> String {
    format!(
        "Pulled {name} ({}) from your machine to {landed}",
        human_size(bytes)
    )
}

/// `1.2 MB` style size for the receipt.
pub fn human_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.1} MB", b / (KB * KB))
    }
}
