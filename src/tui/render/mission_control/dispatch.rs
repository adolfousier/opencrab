//! Mission Control top-level dispatch — paints the backdrop, computes
//! the panel layout, calls each panel renderer, then overlays the help
//! bar (and detail popup, when open).

use super::layout::{self, McLayout};
use super::theme;
use super::{activity_panel, analytics_panel, detail_popup, inbox_panel, schedule_panel};

use crate::tui::app::App;
use crate::tui::app::mission_control::McPanel;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Render Mission Control over the full content area `area`. Inherits
/// the terminal background — no dark wash — to match the `Sessions`
/// and `Help` screens.
pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let McLayout {
        inbox,
        analytics,
        activity,
        schedule,
        help_bar,
    } = layout::compute(area);

    let focus = app.mc.focused_panel;
    inbox_panel::draw(frame, app, inbox, focus == McPanel::Inbox);
    analytics_panel::draw(frame, app, analytics, focus == McPanel::Analytics);
    activity_panel::draw(frame, app, activity, focus == McPanel::Activity);
    schedule_panel::draw(frame, app, schedule, focus == McPanel::Schedule);

    if help_bar.height > 0 {
        draw_help_bar(frame, help_bar);
    }

    if app.mc.detail_open {
        detail_popup::draw(frame, app, area);
    }
}

fn draw_help_bar(frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(" Tab", theme::help_bar_style()),
        Span::styled(": switch panel  ", theme::dim()),
        Span::styled("↑↓", theme::help_bar_style()),
        Span::styled(": navigate  ", theme::dim()),
        Span::styled("Enter", theme::help_bar_style()),
        Span::styled(": detail  ", theme::dim()),
        Span::styled("a", theme::help_bar_style()),
        Span::styled(": apply  ", theme::dim()),
        Span::styled("r", theme::help_bar_style()),
        Span::styled(": reject  ", theme::dim()),
        Span::styled("Esc", theme::help_bar_style()),
        Span::styled(": close", theme::dim()),
    ]);
    let bar = Paragraph::new(line);
    frame.render_widget(bar, area);
}
