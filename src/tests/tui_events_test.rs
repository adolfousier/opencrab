use crate::tui::events::*;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

#[test]
fn test_event_handler_creation() {
    let handler = EventHandler::new();
    let sender = handler.sender();
    // Should be able to send events
    assert!(sender.send(TuiEvent::Quit).is_ok());
}

#[test]
fn test_key_matches() {
    let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(key_matches(
        &event,
        KeyCode::Char('c'),
        KeyModifiers::CONTROL
    ));
    assert!(!key_matches(
        &event,
        KeyCode::Char('c'),
        KeyModifiers::empty()
    ));
}

#[test]
fn test_quit_key() {
    let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(keys::is_quit(&event));

    let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty());
    assert!(!keys::is_quit(&event));
}

#[test]
fn test_submit_key() {
    // Plain Enter sends
    let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::empty());
    assert!(keys::is_submit(&event));

    // Ctrl+Enter also sends (backwards compat)
    let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(keys::is_submit(&event));

    // Alt+Enter does NOT send (it inserts newline)
    let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
    assert!(!keys::is_submit(&event));
    assert!(keys::is_newline(&event));
}
