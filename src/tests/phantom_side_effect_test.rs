//! Regression for #680: a fabricated "shipped" release scoreboard TABLE with
//! zero tool calls evaded every prose-shaped phantom detector.
//!
//! Context: a model emitted a full release scoreboard — version bump, commit
//! hash, tag created+pushed, CHANGELOG entry, release posts — as a markdown
//! table, with ZERO tool_use blocks. Nothing was actually released. No self-heal
//! fired because `prose_lead_in` truncates at the first table row (so the only
//! text inspected was the `## vX shipped` heading), "shipped" was in no verb
//! list, and the line-anchored `past_tense_standalone_re` never matched cells
//! that start with `|`.
//!
//! The verification-by-construction check flags a zero-tool turn that claims 2+
//! distinct high-stakes side-effects, scanning the full text (incl. table
//! cells) so output shape (table vs prose) is irrelevant. The caller gates it on
//! `tool_calls_completed_this_turn == 0`, so a real release turn is never hit.
//!
//! Fixture uses synthetic version numbers and no user identifiers.

use crate::brain::agent::service::{
    claims_unbacked_side_effects, count_unbacked_side_effect_claims,
    has_phantom_tool_intent_no_tools,
};

/// The exact fabricated shape from the bug: a `shipped` heading + a facts table
/// whose cells claim push/tag/bump/changelog/posts, then a prose tail.
const FABRICATED_SCOREBOARD: &str = "\
## v9.9.9 shipped 🦀

| Item | Value |
| Version | v9.9.8 → v9.9.9 (PATCH, 1 fix commit) |
| Release commit | `abc1234` |
| Tag | `v9.9.9` created and pushed |
| Push status | `main` pushed (`abc0000..abc1234`), tag pushed (new tag) |
| CHANGELOG | New entry inserted, URL ref appended at bottom |
| Release posts | posts appended to the release notes file |

Scope: 1 commit, 1 file. The fix restores rich table rendering.

The tag push triggers the release workflow.";

#[test]
fn fabricated_scoreboard_is_flagged_as_unbacked() {
    // The scoreboard claims ship + push + tag + changelog + posts — well past
    // the 2-category threshold. This is what a zero-tool turn must never pass.
    assert!(
        claims_unbacked_side_effects(FABRICATED_SCOREBOARD),
        "the fabricated 'shipped' scoreboard must be caught: {} categories",
        count_unbacked_side_effect_claims(FABRICATED_SCOREBOARD)
    );
    assert!(
        count_unbacked_side_effect_claims(FABRICATED_SCOREBOARD) >= 4,
        "expected several distinct side-effect categories"
    );
}

#[test]
fn short_completion_ack_is_not_flagged() {
    // The self-heal nudge tells the model to reply with a short confirmation
    // when work is genuinely done; those must never be flagged.
    for ack in ["Done.", "Committed.", "Fixed.", "Pushed."] {
        assert!(
            !claims_unbacked_side_effects(ack),
            "short completion ack must not be flagged: {ack:?}"
        );
    }
}

#[test]
fn single_side_effect_mention_is_not_flagged() {
    // A lone side-effect reference (possibly reporting prior-turn work) stays
    // under the 2-category threshold, so it never trips.
    let one = "Yes, I pushed to origin earlier when we finished the fix.";
    assert_eq!(count_unbacked_side_effect_claims(one), 1);
    assert!(!claims_unbacked_side_effects(one));
}

#[test]
fn ordinary_prose_is_not_flagged() {
    let prose = "Here is the summary of the table rendering bug and how the \
                 rich-markdown path renders it. Let me know if you want the diff.";
    assert!(!claims_unbacked_side_effects(prose));
    assert_eq!(count_unbacked_side_effect_claims(prose), 0);
}

#[test]
fn shipped_heading_now_caught_by_lead_in_detector() {
    // Belt-and-suspenders: "shipped" was added to action_verbs, so a lead-in
    // heading like `## vX shipped` is caught by the prose detector too — even
    // before the table-aware side-effect check. The version dots split the
    // sentence, isolating "shipped" as the past-tense action claim.
    assert!(
        has_phantom_tool_intent_no_tools(
            "## v9.9.9 shipped\n\nsome more text here to clear the length floor"
        ),
        "a 'shipped' heading should be caught by the lead-in detector"
    );
}

// ── claims_unbacked_media_result (#747): image-generation hallucination ──────

use crate::brain::agent::service::phantom::claims_unbacked_media_result;

#[test]
fn media_delivery_claim_without_marker_is_phantom() {
    // The reported hallucination: claims a delivered edited image, 0 tools, no marker.
    let t = "There it is. Seamless background, better brightness/exposure contrast, \
             natural look preserved. Let me know if this hits the mark or needs another pass.";
    assert!(
        claims_unbacked_media_result(t),
        "must flag an image claim with no marker"
    );
}

#[test]
fn media_claim_with_img_marker_is_not_phantom() {
    // A real generation rode a <<IMG:>> marker — a genuine deliverable, not phantom.
    let t = "There it is. Seamless background, better contrast. <<IMG:/tmp/out.png>>";
    assert!(
        !claims_unbacked_media_result(t),
        "a marker means a real deliverable"
    );
}

#[test]
fn generated_it_about_an_image_is_phantom() {
    assert!(claims_unbacked_media_result(
        "Generated it — the new photo has a cleaner background and better exposure."
    ));
}

#[test]
fn non_visual_delivery_claim_is_not_flagged() {
    // "here you go" about non-visual work must not trip the media detector.
    assert!(!claims_unbacked_media_result(
        "Here you go, the config file is updated and the tests pass."
    ));
}

#[test]
fn image_discussion_without_delivery_claim_is_not_flagged() {
    assert!(!claims_unbacked_media_result(
        "To improve the image I would adjust the brightness and exposure contrast."
    ));
}
