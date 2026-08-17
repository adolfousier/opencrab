//! Telegram approval dedup.
//!
//! Moved out of `src/channels/telegram/dedup_approval.rs`: tests live under `src/tests/`,
//! never inline beside the logic they exercise (#1076).

use crate::channels::telegram::dedup_approval::*;

#[test]
fn parse_apply_single() {
    assert_eq!(
        parse_callback("apply:abc123"),
        Some(DedupAction::Apply("abc123".to_string()))
    );
}

#[test]
fn parse_reject_single() {
    assert_eq!(
        parse_callback("reject:xyz"),
        Some(DedupAction::Reject("xyz".to_string()))
    );
}

#[test]
fn parse_apply_all() {
    assert_eq!(parse_callback("apply_all"), Some(DedupAction::ApplyAll));
}

#[test]
fn parse_rejects_empty_id() {
    assert_eq!(parse_callback("apply:"), None);
    assert_eq!(parse_callback("reject:"), None);
}

#[test]
fn parse_rejects_unknown_verb() {
    assert_eq!(parse_callback("approve:abc"), None);
    assert_eq!(parse_callback("garbage"), None);
    assert_eq!(parse_callback(""), None);
}

#[test]
fn parse_id_with_colon_keeps_suffix() {
    // split_once only splits on the first ':', so ids that themselves
    // contain a colon survive intact.
    assert_eq!(
        parse_callback("apply:ab:cd"),
        Some(DedupAction::Apply("ab:cd".to_string()))
    );
}
