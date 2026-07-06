//! Tests for the pre-block dynamic status inputs (context-based only).
//!
//! The standalone pre-block status must be DYNAMIC: the model's own
//! thinking excerpt, or a preview of what the user asked. The hardcoded
//! quip pool was deleted as a regression; these tests pin the surviving
//! pure helpers that feed the dynamic surface.

use crate::channels::telegram::handler::{build_user_message_preview, thinking_status_excerpt};

#[test]
fn user_preview_truncates_and_cleans() {
    let p = build_user_message_preview("how do I add a topic?").expect("preview");
    assert!(p.contains("how do I add a topic"));
    let long = "x".repeat(500);
    let p = build_user_message_preview(&long).expect("preview");
    assert!(p.chars().count() < 200, "preview stays short");
}

#[test]
fn empty_input_yields_no_preview() {
    assert!(build_user_message_preview("").is_none());
    assert!(build_user_message_preview("   ").is_none());
}

#[test]
fn thinking_excerpt_is_dynamic_from_reasoning() {
    let t = thinking_status_excerpt("I need to check the config loader first, then...");
    let t = t.expect("excerpt from non-empty thinking");
    assert!(
        t.contains("config loader"),
        "excerpt carries real context: {t}"
    );
    assert!(thinking_status_excerpt("").is_none());
}
