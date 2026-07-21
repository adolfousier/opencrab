//! Regression for #682: in a group the agent addressed the owner by another
//! member's name — it called the current sender "Adi" because a different user
//! named Adi was in the injected recent-group-history.
//!
//! The current-speaker label must name WHO to reply to and flag history names as
//! other people, and the history block must read as prior context from various
//! senders, so history names can't bleed into who the agent addresses.

use crate::channels::telegram::handler::{frame_group_history, group_current_sender_label};

#[test]
fn current_sender_label_names_the_speaker_and_fences_off_history_names() {
    let label = group_current_sender_label("team room", "Adolfo", " (@adolfodev)", "owner");
    // Names the current sender and their handle + role.
    assert!(label.contains("Adolfo") && label.contains("(@adolfodev)"));
    assert!(label.contains("owner"));
    // Instructs the agent to reply to THIS sender.
    assert!(
        label.to_lowercase().contains("reply to adolfo"),
        "label must tell the agent who to reply to: {label}"
    );
    // Explicitly fences off history names — the crux of #682.
    assert!(
        label.contains("history") && label.to_lowercase().contains("other people"),
        "label must warn that history names are other people: {label}"
    );
}

#[test]
fn label_role_reflects_owner_vs_user() {
    assert!(group_current_sender_label("g", "Carlos", "", "user").contains("(user)"));
    assert!(group_current_sender_label("g", "Adolfo", "", "owner").contains("(owner)"));
}

#[test]
fn history_frame_marks_lines_as_other_senders_not_the_current_person() {
    let lines = "[14:02] Adi: hello everyone\n[14:03] Carlos: hi";
    let framed = frame_group_history(lines, 2);
    assert!(framed.contains(lines), "history content must be preserved");
    assert!(framed.contains("2 messages"));
    // Framing must make clear these are OTHER senders / context, not the
    // person being replied to.
    let lower = framed.to_lowercase();
    assert!(
        lower.contains("various senders") && lower.contains("not the"),
        "history must be framed as prior context from others: {framed}"
    );
    assert!(framed.contains("--- end history ---"));
}
