use crossterm::event::{KeyCode, KeyModifiers};

/// The word-delete chord predicate from `src/tui/app/input.rs`.
/// Extracted here as a pure function so we can test it without a full App.
///
/// The actual guard is:
///   `(code == Backspace && modifiers.intersects(ALT | CONTROL))
///     || (code == Char('w') && modifiers == CONTROL)`
///
/// Meaning: delete the word before the cursor on Alt+Backspace (macOS,
/// when the terminal sends Option-as-Meta), Ctrl+Backspace, or Ctrl+W
/// (the cross-terminal readline fallback for terminals — e.g. Warp's
/// default — that never produce the ALT modifier for Option+Backspace).
fn deletes_word(code: KeyCode, modifiers: KeyModifiers) -> bool {
    (code == KeyCode::Backspace && modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL))
        || (code == KeyCode::Char('w') && modifiers == KeyModifiers::CONTROL)
}

#[test]
fn alt_backspace_deletes_word() {
    assert!(deletes_word(KeyCode::Backspace, KeyModifiers::ALT));
}

#[test]
fn ctrl_backspace_deletes_word() {
    // Some terminals send Ctrl+Backspace for word-erase.
    assert!(deletes_word(KeyCode::Backspace, KeyModifiers::CONTROL));
}

#[test]
fn ctrl_w_deletes_word() {
    // readline word-erase — the universal fallback (Warp, tmux, etc.).
    assert!(deletes_word(KeyCode::Char('w'), KeyModifiers::CONTROL));
}

#[test]
fn plain_backspace_does_not_delete_word() {
    // Bare Backspace removes a single char, not a word.
    assert!(!deletes_word(KeyCode::Backspace, KeyModifiers::empty()));
}

#[test]
fn alt_w_does_not_delete_word() {
    // Only Ctrl+W is the word-erase chord; Alt+W is not.
    assert!(!deletes_word(KeyCode::Char('w'), KeyModifiers::ALT));
}

#[test]
fn ctrl_shift_w_does_not_delete_word() {
    // Exact-match on CONTROL means Ctrl+Shift+W is not the word-erase chord.
    assert!(!deletes_word(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT
    ));
}

#[test]
fn plain_w_is_normal_typing() {
    assert!(!deletes_word(KeyCode::Char('w'), KeyModifiers::empty()));
}
