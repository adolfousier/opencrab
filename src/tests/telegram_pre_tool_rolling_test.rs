//! Tests for the pre-block dynamic status inputs (context-based only).
//!
//! The standalone pre-block status must be DYNAMIC: the model's own
//! thinking excerpt, or a preview of what the user asked. The hardcoded
//! quip pool was deleted as a regression; these tests pin the surviving
//! pure helpers that feed the dynamic surface.

use crate::channels::telegram::handler::thinking_status_excerpt;

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
