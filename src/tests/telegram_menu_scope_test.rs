//! "Not a member here" is not a failure (#839).
//!
//! Global `allowed_users` are admins allowed in every group. Clearing their
//! per-member menu in a group they do not belong to comes back as an invalid-id
//! error, which was logged as a warning: four errors per group across nine
//! groups on every registration pass, burying real failures.
//!
//! The first proposed fix was to stop consulting the global list. That would
//! have regressed the admins who ARE members, leaving them the default menu
//! offering /start. These pin the classification instead.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::channels::telegram::menu_scope::means_not_a_member;

#[test]
fn the_observed_error_is_recognised() {
    assert!(means_not_a_member("USER_ID_INVALID"));
}

#[test]
fn the_error_is_recognised_inside_a_wrapped_message() {
    // teloxide renders these as opaque API strings, so the code arrives
    // embedded in surrounding text rather than as a typed variant.
    assert!(means_not_a_member(
        "Telegram API error: Bad Request: USER_ID_INVALID"
    ));
}

#[test]
fn the_other_non_membership_codes_are_recognised() {
    for code in ["PARTICIPANT_ID_INVALID", "USER_NOT_PARTICIPANT"] {
        assert!(means_not_a_member(code), "must recognise: {code}");
    }
}

#[test]
fn matching_is_case_insensitive() {
    assert!(means_not_a_member("bad request: user_id_invalid"));
}

#[test]
fn real_failures_still_warn() {
    // The point of the change is that genuine faults stay visible. If any of
    // these were swallowed the log would go quiet on problems worth seeing.
    for err in [
        "CHAT_NOT_FOUND",
        "BOT_WAS_KICKED",
        "Retry after 30",
        "connection closed before message completed",
        "TOO_MANY_REQUESTS",
        "CHAT_ADMIN_REQUIRED",
    ] {
        assert!(!means_not_a_member(err), "must stay a warning: {err}");
    }
}

#[test]
fn an_empty_error_is_not_a_membership_answer() {
    assert!(!means_not_a_member(""));
}
