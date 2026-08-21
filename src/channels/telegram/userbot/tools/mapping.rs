//! Pure param mapping for the tool plane — no I/O, fully testable.
//!
//! Everything gramers can't express as a builder call lands here as data
//! first: date strings become `DateTime<FixedOffset>`, over-fetched pages
//! become `(page, has_more)`, chat id strings become Bot API dialog ids.

use anyhow::{Result, bail};
use chrono::{DateTime, FixedOffset, NaiveDate};

/// Parse a chat filter date: full RFC 3339, or a bare `YYYY-MM-DD`
/// (midnight UTC). Anything else is a caller error, not a default.
pub(crate) fn parse_date(s: &str) -> Result<DateTime<FixedOffset>> {
    let s = s.trim();
    if s.len() == 10 {
        let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|e| anyhow::anyhow!("invalid date {s:?}: {e}"))?;
        return Ok(d
            .and_hms_opt(0, 0, 0)
            .expect("midnight is always valid")
            .and_utc()
            .fixed_offset());
    }
    DateTime::parse_from_rfc3339(s).map_err(|e| anyhow::anyhow!("invalid datetime {s:?}: {e}"))
}

/// Parse a Bot API dialog id (the `-100…` / `-…` / `…` forms). Rejects
/// values outside Telegram's id ranges — better an error now than a
/// silent wrong-chat fetch later.
pub(crate) fn parse_bot_api_chat_id(s: &str) -> Result<i64> {
    let s = s.trim();
    let id: i64 = s
        .parse()
        .map_err(|e| anyhow::anyhow!("chat id {s:?} is not numeric: {e}"))?;
    // Range check without importing session types into a pure module:
    // PeerId::from_bot_api_dialog_id is the authority, but it lives in
    // the feature-gated dep; the ranges it encodes are stable and public
    // (core.telegram.org/api/bots/ids).
    let in_range = (1..=0xffffffffff).contains(&id)
        || (-999999999999..=-1).contains(&id)
        || (-1997852516352..=-1000000000001).contains(&id)
        || (-4000000000000..=-2002147483649).contains(&id);
    if !in_range {
        bail!("chat id {s:?} is outside Telegram's valid ranges");
    }
    Ok(id)
}

/// Page-shape helper: callers fetch `limit + 1` items, then this splits
/// them into the page and the `has_more` flag the MCP envelope promises.
pub(crate) fn truncate_with_more(
    mut items: Vec<serde_json::Value>,
    limit: usize,
) -> (Vec<serde_json::Value>, bool) {
    let has_more = items.len() > limit;
    items.truncate(limit);
    (items, has_more)
}

/// Narrow a user-supplied message id (JSON number, i64) to the i32
/// gramers 0.10 takes. Telegram message ids are far below i32::MAX in
/// practice; a value that isn't is a caller error worth naming.
pub(crate) fn narrow_message_id(id: i64) -> Result<i32> {
    i32::try_from(id).map_err(|_| anyhow::anyhow!("message id {id} is out of range"))
}

/// Normalize a phone number for `contacts.importContacts`: strip
/// spaces/dashes/parens, keep an optional leading `+`, require 7..=15
/// digits. E.164 in, E.164 out; anything else is a caller error.
pub(crate) fn normalize_phone(s: &str) -> Result<String> {
    let cleaned: String = s
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '(' | ')' | '.'))
        .collect();
    let plus = cleaned.starts_with('+');
    let digits = cleaned.trim_start_matches('+');
    if !digits.chars().all(|c| c.is_ascii_digit()) || !(7..=15).contains(&digits.len()) {
        bail!("phone {s:?} is not a plausible phone number");
    }
    Ok(format!("{}{}", if plus { "+" } else { "" }, digits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_parse_bare_and_full() {
        assert_eq!(parse_date("2026-01-01").unwrap().timestamp(), 1_767_225_600);
        assert_eq!(
            parse_date("2026-01-01T10:00:00Z").unwrap().timestamp(),
            1_767_261_600
        );
        assert!(parse_date("yesterday").is_err());
        assert!(parse_date("").is_err());
    }

    #[test]
    fn chat_ids_parse_and_reject() {
        assert_eq!(parse_bot_api_chat_id("123").unwrap(), 123);
        assert_eq!(
            parse_bot_api_chat_id("-1001234567890").unwrap(),
            -1001234567890
        );
        assert_eq!(parse_bot_api_chat_id("-999").unwrap(), -999);
        assert!(parse_bot_api_chat_id("0").is_err());
        assert!(parse_bot_api_chat_id("abc").is_err());
        assert!(parse_bot_api_chat_id("99999999999999").is_err());
    }

    #[test]
    fn truncate_flags_more_only_when_overflowing() {
        let items = (0..21).map(|i| serde_json::json!(i)).collect();
        let (page, more) = truncate_with_more(items, 20);
        assert_eq!(page.len(), 20);
        assert!(more);

        let items = (0..15).map(|i| serde_json::json!(i)).collect();
        let (page, more) = truncate_with_more(items, 20);
        assert_eq!(page.len(), 15);
        assert!(!more);
    }

    #[test]
    fn message_ids_narrow_or_error() {
        assert_eq!(narrow_message_id(42).unwrap(), 42);
        assert_eq!(narrow_message_id(i32::MAX as i64).unwrap(), i32::MAX);
        assert!(narrow_message_id(i32::MAX as i64 + 1).is_err());
        assert!(narrow_message_id(-1).is_ok());
    }

    #[test]
    fn phones_normalize_and_reject() {
        assert_eq!(
            normalize_phone("+254 712-345.678").unwrap(),
            "+254712345678"
        );
        assert_eq!(normalize_phone("254712345678").unwrap(), "254712345678");
        assert_eq!(
            normalize_phone("(254) 7123456789").unwrap(),
            "2547123456789"
        );
        assert!(normalize_phone("+254712345678901234").is_err()); // too long
        assert!(normalize_phone("12345").is_err()); // too short
        assert!(normalize_phone("+25471234567a").is_err()); // letters
        assert!(normalize_phone("").is_err());
    }
}

/// Resolve where a media download lands. Explicit paths must be
/// absolute, inside `home`, and free of `..` components — traversal is
/// refused at the boundary. The default is
/// `<home>/.opencrabs/userbot_media/<sanitized chat>/msg-<id>.bin`
/// (deterministic; the envelope returns the real path).
pub(crate) fn resolve_download_path(
    explicit: Option<&str>,
    chat: &str,
    message_id: i64,
    home: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    use std::path::{Component, PathBuf};

    let sanitize = |c: char| {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
            c
        } else {
            '_'
        }
    };
    let path = match explicit {
        Some(raw) => {
            let raw = raw.trim();
            let p = PathBuf::from(raw);
            if !p.is_absolute() {
                return Err(format!("download path must be absolute: {raw}"));
            }
            if p.components().any(|c| matches!(c, Component::ParentDir)) {
                return Err(format!("download path must not contain '..': {raw}"));
            }
            p
        }
        None => home
            .join(".opencrabs")
            .join("userbot_media")
            .join(chat.chars().map(sanitize).collect::<String>())
            .join(format!("msg-{message_id}.bin")),
    };
    if !path.starts_with(home) {
        return Err(format!(
            "download path must stay under $HOME: {}",
            path.display()
        ));
    }
    Ok(path)
}

#[cfg(test)]
mod download_path_tests {
    use super::*;

    #[test]
    fn default_path_is_deterministic_under_home() {
        let home = std::path::Path::new("/home/tester");
        let p = resolve_download_path(None, "-1001234567890", 42, home).unwrap();
        assert_eq!(
            p,
            home.join(".opencrabs/userbot_media/-1001234567890/msg-42.bin")
        );
    }

    #[test]
    fn chat_component_is_sanitized() {
        let home = std::path::Path::new("/h");
        let p = resolve_download_path(None, "a/b c@x", 1, home).unwrap();
        assert!(p.starts_with(home.join(".opencrabs/userbot_media/a_b_c_x")));
    }

    #[test]
    fn explicit_paths_are_validated() {
        let home = std::path::Path::new("/home/tester");
        assert!(resolve_download_path(Some("/home/tester/x.mp4"), "c", 1, home).is_ok());
        assert!(resolve_download_path(Some("rel/x.mp4"), "c", 1, home).is_err());
        assert!(resolve_download_path(Some("/home/../etc/x"), "c", 1, home).is_err());
        assert!(resolve_download_path(Some("/tmp/x.mp4"), "c", 1, home).is_err());
    }
}
