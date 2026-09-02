//! No-op stand-ins compiled when the `rtk` feature is off, so callers keep
//! one code path: nothing is available, nothing is rewritten.

pub async fn is_rtk_available() -> bool {
    false
}

pub async fn rewrite_command(_command: &str) -> Option<String> {
    None
}

pub fn warm_up() {}
