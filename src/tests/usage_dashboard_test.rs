use super::*;
use ratatui::layout::Rect;

#[test]
fn test_default_state() {
    let s = DashboardState::default();
    assert_eq!(s.period, Period::AllTime);
    assert_eq!(s.focused_card, 0);
}

#[test]
fn test_focus_cycling() {
    let mut s = DashboardState::default();
    assert_eq!(s.focused_card, 0);
    s.focus_next();
    assert_eq!(s.focused_card, 1);
    s.focus_next();
    assert_eq!(s.focused_card, 2);
    s.focus_next();
    assert_eq!(s.focused_card, 3);
    s.focus_next();
    assert_eq!(s.focused_card, 4);
    s.focus_next();
    assert_eq!(s.focused_card, 5);
    s.focus_next();
    assert_eq!(s.focused_card, 0); // wraps

    s.focus_prev();
    assert_eq!(s.focused_card, 5); // wraps back
    s.focus_prev();
    assert_eq!(s.focused_card, 4);
}

#[test]
fn test_set_period() {
    let mut s = DashboardState::default();
    assert!(s.set_period(Period::Today));
    assert!(!s.set_period(Period::Today)); // same, no change
    assert!(s.set_period(Period::Week));
}

#[test]
fn test_centered_rect_large_terminal() {
    let area = Rect::new(0, 0, 200, 60);
    let r = centered_rect(area);
    assert_eq!(r.width, 150); // 75% of 200
    assert_eq!(r.height, 45); // 75% of 60
    assert_eq!(r.x, 25); // centered
    assert_eq!(r.y, 7); // centered (rounding)
}

#[test]
fn test_centered_rect_small_terminal() {
    let area = Rect::new(0, 0, 80, 24);
    let r = centered_rect(area);
    assert_eq!(r.width, 60); // 75% of 80 = 60, meets floor of 60
    assert_eq!(r.height, 20); // max(75%=18, floor=20) = 20, capped at 22
}

#[test]
fn test_centered_rect_tiny_terminal() {
    let area = Rect::new(0, 0, 40, 15);
    let r = centered_rect(area);
    // Should not exceed area
    assert!(r.width <= area.width);
    assert!(r.height <= area.height);
    assert!(r.x >= area.x);
    assert!(r.y >= area.y);
}
