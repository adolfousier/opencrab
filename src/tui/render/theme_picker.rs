//! Interactive theme picker dialog (#1371).
//!
//! Bare `/theme` opens the picker; `/theme list` keeps the text surface
//! (owner ruling: the dialog never replaces the proven text command —
//! if the two ever fight, the dialog loses).
//!
//! Core UX is preview-on-highlight: ↑/↓ moves through the roster and the
//! caller applies [`PickerAction::Preview`] via `theme::set`, so the whole
//! UI (this dialog included — its chrome resolves through `theme::role()`)
//! recolors live under the cursor. Esc reverts to the theme that was
//! active on open; Enter applies and lets the caller persist.
//!
//! The state machine is pure: `handle_key` never touches the global
//! theme slot itself, it only returns actions. That keeps the unit tests
//! free of cross-test races on the process-wide `ACTIVE` lock and puts
//! every global mutation in one place (`state.rs`'s key handler).

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::presets;
use super::theme::{self, Theme};
use super::user_themes;
use crate::tui::events::keys;

/// One selectable row in the picker. `theme: None` marks a rejected user
/// preset — shown disabled with its reason, never previewable/applicable.
#[derive(Debug)]
pub struct ThemePickerItem {
    pub name: &'static str,
    pub theme: Option<&'static Theme>,
    pub reason: Option<String>,
    pub is_user: bool,
}

/// What the key handler decided. The caller executes every global effect:
/// `Preview`/`Apply` call `theme::set`, `Apply` also persists to
/// `[tui.theme]`, `Cancel` reverts to `ThemePickerState::origin`.
// `Theme` intentionally has no `PartialEq` (identity is the static slot),
// so action equality in tests goes through `matches!` + `std::ptr::eq`.
#[derive(Debug, Clone, Copy)]
pub enum PickerAction {
    None,
    Preview(&'static Theme),
    Apply(&'static Theme),
    Cancel,
}

/// Dialog state. `origin` is the theme active when the picker opened —
/// the Esc-revert target, and the row that keeps the ● applied marker
/// while other rows are merely previewed.
#[derive(Debug)]
pub struct ThemePickerState {
    pub items: Vec<ThemePickerItem>,
    pub selected: usize,
    pub origin: &'static Theme,
    pub scroll_offset: usize,
}

impl ThemePickerState {
    /// Open the picker: rescan user presets (hot-load semantics, same as
    /// `/theme list`) and land the cursor on the currently active theme.
    pub fn open() -> Self {
        let report = user_themes::reload();
        let origin = theme::active();
        let (items, selected) = build_items(presets::built_ins(), &report, origin.name);
        Self {
            items,
            selected,
            origin,
            scroll_offset: 0,
        }
    }

    /// True when the highlighted row is a rejected (non-selectable) theme.
    /// Reserved for the pending status-line work; kept visible to clippy.
    #[allow(dead_code)]
    fn selected_is_rejected(&self) -> bool {
        self.items.get(self.selected).is_some_and(|i| i.theme.is_none())
    }

    /// Move toward `step`, skipping rejected rows, clamping at the ends.
    fn move_selection(&mut self, step: isize) {
        if self.items.is_empty() {
            return;
        }
        let mut next = self.selected as isize + step;
        while next >= 0
            && (next as usize) < self.items.len()
            && self.items[next as usize].theme.is_none()
        {
            next += step;
        }
        if next >= 0 && (next as usize) < self.items.len() {
            self.selected = next as usize;
        }
    }

    /// Keep the selected row inside the visible window.
    fn scroll_into_view(&mut self, visible: usize) {
        if visible == 0 {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible {
            self.scroll_offset = self.selected + 1 - visible;
        }
    }

    /// Pure key handling: returns the action for the caller to execute.
    pub fn handle_key(&mut self, event: &KeyEvent, visible_rows: usize) -> PickerAction {
        if keys::is_cancel(event) || event.code == KeyCode::Char('q') {
            return PickerAction::Cancel;
        }
        if keys::is_enter(event) {
            return match self.items.get(self.selected).and_then(|i| i.theme) {
                Some(t) => PickerAction::Apply(t),
                None => PickerAction::None, // Enter on a rejected row: no-op
            };
        }
        let step = if keys::is_up(event) || event.code == KeyCode::Char('k') {
            -1
        } else if keys::is_down(event) || event.code == KeyCode::Char('j') {
            1
        } else if keys::is_page_up(event) {
            -8
        } else if keys::is_page_down(event) {
            8
        } else {
            return PickerAction::None;
        };
        let prev = self.selected;
        self.move_selection(step);
        self.scroll_into_view(visible_rows.max(1));
        if self.selected == prev {
            // Movement clamped at the list edge (or no valid row in that
            // direction): selection unchanged, no preview re-emit.
            return PickerAction::None;
        }
        match self.items.get(self.selected).and_then(|i| i.theme) {
            Some(t) => PickerAction::Preview(t),
            None => PickerAction::None,
        }
    }
}

/// Pure item builder: built-ins first (registry order), then valid user
/// presets, then rejected files as disabled rows with their reason.
/// Returns the items plus the index of `active_name` (0 if not found —
/// impossible in practice since the origin theme is always in the list).
fn build_items(
    builtins: &[&'static Theme],
    report: &user_themes::LoadReport,
    active_name: &str,
) -> (Vec<ThemePickerItem>, usize) {
    let mut items: Vec<ThemePickerItem> = Vec::new();
    let mut selected = 0;
    for t in builtins {
        if t.name == active_name {
            selected = items.len();
        }
        items.push(ThemePickerItem {
            name: t.name,
            theme: Some(t),
            reason: None,
            is_user: false,
        });
    }
    for t in &report.themes {
        if t.name == active_name {
            selected = items.len();
        }
        items.push(ThemePickerItem {
            name: t.name,
            theme: Some(t),
            reason: None,
            is_user: true,
        });
    }
    for r in &report.rejected {
        items.push(ThemePickerItem {
            name: "",
            theme: None,
            reason: Some(format!("{} — {}", r.file, r.reason)),
            is_user: true,
        });
    }
    (items, selected)
}

/// Centered popup over the live UI. The base chat stays fully rendered
/// underneath (frame.rs renders it before calling this) so previewing a
/// theme recolors the actual interface around the dialog.
pub fn draw(f: &mut Frame, area: Rect, state: &ThemePickerState) {
    // 46 cols fits `▶ ● solarized-light (user)` comfortably; 3 border rows
    // + 2 hint rows chrome; list rows capped by the roster size.
    let width = 48u16;
    let height = (state.items.len() as u16 + 6).min(area.height.saturating_sub(4)).max(9);
    let popup = centered_rect(area, width, height);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Themes ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::role(theme::Role::Accent)));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Hint footer (last row), list above it.
    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    let visible = rows[0].height as usize;

    let active = theme::active().name;
    let mut list_state = ListState::default();
    let items: Vec<ListItem> = state
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let cursor = if idx == state.selected { "▶ " } else { "  " };
            let line = match (&item.theme, &item.reason) {
                (Some(t), _) => {
                    let applied = if t.name == state.origin.name { "● " } else { "  " };
                    let tag = if item.is_user { " (user)" } else { "" };
                    let style = if idx == state.selected {
                        Style::default().fg(theme::role(theme::Role::Accent))
                    } else if t.name == active {
                        Style::default().fg(theme::role(theme::Role::AccentTeal))
                    } else {
                        Style::default().fg(theme::role(theme::Role::TextPrimary))
                    };
                    Line::from(Span::styled(format!("{cursor}{applied}{}{tag}", t.name), style))
                }
                (None, Some(reason)) => Line::from(Span::styled(
                    format!("  ✗ {reason}"),
                    Style::default().fg(theme::role(theme::Role::TextMuted)),
                )),
                (None, None) => Line::from("  ✗"),
            };
            ListItem::new(line)
        })
        .collect();

    // Render-time scroll clamp for resizes, then window the slice.
    let mut offset = state.scroll_offset.min(visible.saturating_sub(1));
    if state.selected < offset {
        offset = state.selected;
    } else if visible > 0 && state.selected >= offset + visible {
        offset = state.selected + 1 - visible;
    }
    list_state.select(Some(state.selected));

    let list = List::new(items)
        .style(Style::default().fg(theme::role(theme::Role::TextPrimary)));
    // ratatui reads the scroll window back out of the state after rendering,
    // so hand it our per-frame offset through the real offset slot.
    *list_state.offset_mut() = offset;
    f.render_stateful_widget(list, rows[0], &mut list_state);

    let hints = Paragraph::new("↑/↓ preview · Enter apply · Esc cancel")
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme::role(theme::Role::TextMuted)));
    f.render_widget(hints, rows[1]);
}

/// Fixed-size popup centered in `area`, clamped to fit.
fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.width.saturating_sub(width) / 2;
    let y = area.height.saturating_sub(height) / 2;
    Rect {
        x: area.x + x,
        y: area.y + y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn item(name: &'static str, t: Option<&'static Theme>, is_user: bool) -> ThemePickerItem {
        ThemePickerItem {
            name,
            theme: t,
            reason: t.is_none().then(|| "rejected.toml — bad hex".to_string()),
            is_user,
        }
    }

    /// Hand-built state: `open()` touches process-wide globals (preset scan,
    /// active theme) and would race across parallel tests.
    fn fixture() -> ThemePickerState {
        ThemePickerState {
            items: vec![
                item("crab-dark", Some(&theme::CRAB_DARK), false),
                item("dracula", Some(&presets::DRACULA), false),
                item("", None, true),
                item("solarized-dark", Some(&presets::SOLARIZED_DARK), false),
            ],
            selected: 0,
            origin: &theme::CRAB_DARK,
            scroll_offset: 0,
        }
    }

    fn expect_preview(action: PickerAction, want: &'static Theme) {
        match action {
            PickerAction::Preview(t) => assert!(std::ptr::eq(t, want), "got {:?}", t.name),
            other => panic!("expected Preview, got {other:?}"),
        }
    }

    fn expect_noop(action: PickerAction) {
        assert!(matches!(action, PickerAction::None), "got {action:?}");
    }

    fn expect_cancel(action: PickerAction) {
        assert!(matches!(action, PickerAction::Cancel), "got {action:?}");
    }

    #[test]
    fn down_previews_next_valid_row() {
        let mut s = fixture();
        expect_preview(s.handle_key(&key(KeyCode::Down), 10), &presets::DRACULA);
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn navigation_skips_rejected_rows_both_ways() {
        let mut s = fixture();
        s.selected = 1; // dracula; row 2 is rejected
        expect_preview(s.handle_key(&key(KeyCode::Down), 10), &presets::SOLARIZED_DARK);
        assert_eq!(s.selected, 3);
        expect_preview(s.handle_key(&key(KeyCode::Up), 10), &presets::DRACULA);
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn enter_applies_selected_row() {
        let mut s = fixture();
        s.selected = 1;
        match s.handle_key(&key(KeyCode::Enter), 10) {
            PickerAction::Apply(t) => assert!(std::ptr::eq(t, &presets::DRACULA)),
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn enter_on_rejected_row_is_noop() {
        let mut s = fixture();
        s.selected = 2;
        expect_noop(s.handle_key(&key(KeyCode::Enter), 10));
    }

    #[test]
    fn esc_and_q_cancel() {
        let mut s = fixture();
        expect_cancel(s.handle_key(&key(KeyCode::Esc), 10));
        expect_cancel(s.handle_key(&key(KeyCode::Char('q')), 10));
    }

    #[test]
    fn movement_clamps_at_list_ends() {
        let mut s = fixture();
        expect_noop(s.handle_key(&key(KeyCode::Up), 10));
        assert_eq!(s.selected, 0);
        s.selected = 3;
        expect_noop(s.handle_key(&key(KeyCode::Down), 10));
        assert_eq!(s.selected, 3);
    }

    #[test]
    fn empty_list_movement_is_safe() {
        let mut s = ThemePickerState {
            items: vec![],
            selected: 0,
            origin: &theme::CRAB_DARK,
            scroll_offset: 0,
        };
        expect_noop(s.handle_key(&key(KeyCode::Down), 10));
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn build_items_lands_cursor_on_active_theme() {
        let report = user_themes::LoadReport {
            themes: vec![],
            rejected: vec![],
        };
        let (items, selected) =
            build_items(&[&theme::CRAB_DARK, &presets::DRACULA], &report, "dracula");
        assert_eq!(selected, 1);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn build_items_surfaces_rejected_files_with_reason() {
        let report = user_themes::LoadReport {
            themes: vec![],
            rejected: vec![user_themes::RejectedPreset {
                file: "broken.toml".to_string(),
                reason: "invalid hex".to_string(),
            }],
        };
        let (items, selected) = build_items(&[&theme::CRAB_DARK], &report, "crab-dark");
        assert_eq!(selected, 0);
        assert_eq!(items.len(), 2);
        assert!(items[1].theme.is_none());
        let reason = items[1].reason.as_deref().unwrap_or_default();
        assert!(reason.contains("broken.toml") && reason.contains("invalid hex"));
    }

    #[test]
    fn scroll_window_follows_selection() {
        let mut s = fixture();
        for _ in 0..3 {
            s.handle_key(&key(KeyCode::Down), 2); // 2-row visible window
        }
        assert_eq!(s.selected, 3);
        assert!(s.selected >= s.scroll_offset);
        assert!(s.selected < s.scroll_offset + 2);
    }
}
