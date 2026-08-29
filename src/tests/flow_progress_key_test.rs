//! Repeated progress lines supersede instead of stacking (#982).
//!
//! A failing turn used to post nine lines for two facts: five
//! `nudge 1/5 .. 5/5` and one per fallback attempt. Everything else in that
//! block already rewrites itself (a tool row flips its icon, the footer
//! rewrites ctx and tok/s), so these were the exception.

use crate::channels::telegram::flow::progress_key;

#[test]
fn every_nudge_attempt_shares_one_key() {
    let keys: Vec<_> = (1..=5)
        .map(|n| {
            progress_key(&format!(
                "🔧 Model reasoned without answering — nudge {n}/5"
            ))
        })
        .collect();
    assert!(keys.iter().all(|k| *k == Some("empty-answer-nudge")));
}

#[test]
fn every_fallback_attempt_shares_one_key() {
    for p in ["zhipu/glm-4.5", "claude-cli/opus-5", "minimax/MiniMax-M3"] {
        assert_eq!(
            progress_key(&format!(
                "🔧 Trying fallback '{p}' for empty-reasoning recovery..."
            )),
            Some("fallback-attempt")
        );
    }
}

#[test]
fn nudges_and_fallbacks_do_not_collapse_into_each_other() {
    // They are two distinct facts and must occupy two lines, not one.
    assert_ne!(
        progress_key("🔧 Model reasoned without answering — nudge 5/5"),
        progress_key("🔧 Trying fallback 'zhipu/glm-4.5' for empty-reasoning recovery...")
    );
}

#[test]
fn ordinary_narration_always_appends() {
    // Returning a key here would silently eat the model's actual narration.
    for text in [
        "Let me check the schema before answering.",
        "Done. The table is in Volume 2.",
        "",
        "🔧 Now using zhipu/glm-4.5",
    ] {
        assert_eq!(progress_key(text), None, "must not supersede: {text:?}");
    }
}

#[test]
fn the_leading_emoji_is_not_required() {
    // The key is derived from the message, not its decoration, so a change of
    // icon cannot silently turn superseding back into stacking.
    assert_eq!(
        progress_key("Model reasoned without answering — nudge 2/5"),
        Some("empty-answer-nudge")
    );
}

#[test]
fn provider_retries_also_supersede() {
    assert_eq!(
        progress_key("⏳ Retry 2/5 — connection reset"),
        Some("provider-retry")
    );
}

#[test]
fn mermaid_regen_attempts_share_one_key() {
    // The regen counter (#37) supersedes in place the same way the
    // empty-answer nudge counter does.
    let keys: Vec<_> = (1..=3)
        .map(|n| progress_key(&format!("🔧 Mermaid render failed — regen {n}/3")))
        .collect();
    assert!(keys.iter().all(|k| *k == Some("mermaid-regen")));
    // No emoji: the key is derived from the message, not its decoration.
    assert_eq!(
        progress_key("Mermaid render failed — regen 1/3"),
        Some("mermaid-regen")
    );
}
