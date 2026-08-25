//! Which binary a channel-triggered restart execs (#1130).
//!
//! `/evolve` from Telegram on Linux failed with
//! `exec() failed: No such file or directory (os error 2)` on every run. The
//! download and the swap were fine; the restart resolved its target from
//! scratch via `current_exe()`, which reads `/proc/self/exe` back as
//! `"<path> (deleted)"` once evolve has unlinked the old inode. Rust returns
//! that suffix verbatim, so the process execs a path that does not exist.
//!
//! Two guards already existed and the channel path used neither: the
//! `RestartReady` event carries the pre-swap path, and `strip_deleted_marker`
//! cleans a poisoned one. `restart_target` is where both now apply, so these
//! pin it directly rather than the helper the buggy path never called.

use crate::channels::commands::restart_target;
use std::path::PathBuf;

/// The producer's path is captured BEFORE the unlink, so it wins outright,
/// even when `current_exe()` has already gone stale underneath it. This is the
/// exact shape of the field the handler used to discard with `{ .. }`.
#[test]
fn prefers_producer_path_over_poisoned_current_exe() {
    assert_eq!(
        restart_target(
            Some(PathBuf::from("/usr/local/bin/opencrabs")),
            PathBuf::from("/usr/local/bin/opencrabs (deleted)"),
        ),
        PathBuf::from("/usr/local/bin/opencrabs"),
    );
}

/// The cargo-install branch reports `binary_path: None` on purpose, so
/// preferring the event path alone would not save it. The fallback has to
/// strip, or that branch keeps execing the literal `"… (deleted)"`.
#[test]
fn strips_deleted_marker_when_producer_gave_no_path() {
    assert_eq!(
        restart_target(None, PathBuf::from("/usr/local/bin/opencrabs (deleted)")),
        PathBuf::from("/usr/local/bin/opencrabs"),
    );
}

/// Evolving from an already-unlinked inode stacks the suffix; a VPS was
/// observed carrying two. One strip would still resolve to a junk file.
#[test]
fn strips_stacked_deleted_markers() {
    assert_eq!(
        restart_target(
            None,
            PathBuf::from("/usr/local/bin/opencrabs (deleted) (deleted)"),
        ),
        PathBuf::from("/usr/local/bin/opencrabs"),
    );
}

/// macOS resolves via `_NSGetExecutablePath` and never appends the marker,
/// which is why the bug was invisible on the Mac TUI. Stripping must be a
/// no-op there, not a truncation.
#[test]
fn clean_current_exe_passes_through_unchanged() {
    assert_eq!(
        restart_target(None, PathBuf::from("/opt/homebrew/bin/opencrabs")),
        PathBuf::from("/opt/homebrew/bin/opencrabs"),
    );
}

/// A path that merely mentions "deleted" mid-string is a real directory name,
/// not a kernel marker. Only the trailing `" (deleted)"` is stripped.
#[test]
fn interior_deleted_text_is_not_stripped() {
    assert_eq!(
        restart_target(None, PathBuf::from("/srv/deleted/bin/opencrabs")),
        PathBuf::from("/srv/deleted/bin/opencrabs"),
    );
}

/// Defence in depth: if a producer ever hands over a path it resolved AFTER
/// the swap, cleaning it costs nothing and keeps the exec valid.
#[test]
fn strips_marker_from_the_preferred_path_too() {
    assert_eq!(
        restart_target(
            Some(PathBuf::from("/usr/local/bin/opencrabs (deleted)")),
            PathBuf::from("/usr/local/bin/opencrabs"),
        ),
        PathBuf::from("/usr/local/bin/opencrabs"),
    );
}
