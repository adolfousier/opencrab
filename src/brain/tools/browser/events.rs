//! CDP event ring buffer (#1189).
//!
//! The browser stack is otherwise purely imperative: `browser_wait` polls
//! on a timer and nothing subscribes to CDP events, so anything
//! spontaneous — a JS dialog freezing the page, a download starting, a
//! renderer crash, a popup — is invisible until it breaks the next
//! action with an opaque timeout.
//!
//! This module keeps a bounded ring of the last [`RING_CAPACITY`]
//! high-value events (dialog opening, download starting, popup target
//! created, target crashed, inspector detached), timestamped, and
//! surfaces them as a compact `recent_events:` line appended to browser
//! tool results. The line is DRAINED on read: each event surfaces
//! exactly once, in the next tool result after it fired — the spec's
//! "cleared after reporting" clause.
//!
//! Dialog RESPONSE stays an explicit model decision (dismiss/accept via
//! a handler call). The summary makes the agent aware; policy decides
//! the action. No silent auto-dismissal.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chromiumoxide::cdp::CdpEvent;
use futures::StreamExt;

/// How many events the ring keeps. ~10 per the spec: enough to cover a
/// burst during one action, small enough that the surfaced line stays
/// readable.
pub const RING_CAPACITY: usize = 10;

/// One captured CDP event, already reduced to a display string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserEvent {
    /// Local wall-clock time, HH:MM:SS.
    pub ts: String,
    /// Human summary, e.g. `js-dialog opened (confirm): "Delete?"`.
    pub summary: String,
}

/// Bounded, thread-safe event ring. Cloned cheaply via `Arc` so the
/// per-page listener tasks and every browser tool share one log.
#[derive(Debug, Clone, Default)]
pub struct EventLog {
    inner: Arc<Mutex<VecDeque<BrowserEvent>>>,
}

impl EventLog {
    pub fn push(&self, summary: String) {
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        let mut guard = self.inner.lock().unwrap();
        if guard.len() >= RING_CAPACITY {
            guard.pop_front();
        }
        guard.push_back(BrowserEvent { ts, summary });
    }

    /// Drain all pending events (spec: cleared after reporting).
    pub fn take(&self) -> Vec<BrowserEvent> {
        let mut guard = self.inner.lock().unwrap();
        guard.drain(..).collect()
    }

    /// Format the drained events as the `recent_events:` line, or `None`
    /// when nothing pending. Example:
    /// `recent_events: [21:14:03 js-dialog opened (confirm), 21:14:04 download started: invoice.pdf]`
    pub fn take_formatted(&self) -> Option<String> {
        let events = self.take();
        if events.is_empty() {
            return None;
        }
        let rendered: Vec<String> = events
            .iter()
            .map(|e| format!("{} {}", e.ts, e.summary))
            .collect();
        Some(format!("recent_events: [{}]", rendered.join(", ")))
    }

    /// Current pending count without draining (tests).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}

/// Reduce a raw CDP event to a display summary, keeping only the
/// high-value set. Pure function over the umbrella enum so tests can
/// drive it without a live browser.
pub fn map_cdp_event(event: &CdpEvent) -> Option<String> {
    match event {
        CdpEvent::PageJavascriptDialogOpening(d) => {
            let kind = dialog_kind(d);
            let msg = truncate(&d.message, 40);
            Some(format!("js-dialog opened ({kind}): \"{msg}\""))
        }
        CdpEvent::BrowserDownloadWillBegin(d) => {
            let name = d.suggested_filename.clone();
            let name = if name.is_empty() {
                truncate(&d.url, 40)
            } else {
                name
            };
            Some(format!("download started: {name}"))
        }
        CdpEvent::TargetTargetCreated(t) => {
            let kind = t.target_info.r#type.clone();
            // Only surface popup-ish targets; same-page worker/iframe
            // targets fire constantly and would be noise.
            if matches!(kind.as_str(), "page" | "background_page" | "webview") {
                Some(format!("new target opened ({kind})"))
            } else {
                None
            }
        }
        CdpEvent::TargetTargetCrashed(_) => Some("renderer target crashed".into()),
        CdpEvent::InspectorDetached(d) => Some(format!("inspector detached ({})", d.reason)),
        _ => None,
    }
}

/// Dialog type label with a sane fallback for unknown enum values.
fn dialog_kind(
    d: &chromiumoxide::cdp::browser_protocol::page::EventJavascriptDialogOpening,
) -> String {
    use chromiumoxide::cdp::browser_protocol::page::DialogType;
    match d.r#type {
        DialogType::Alert => "alert".into(),
        DialogType::Confirm => "confirm".into(),
        DialogType::Prompt => "prompt".into(),
        DialogType::Beforeunload => "beforeunload".into(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// Append a drained `recent_events:` line to a tool result text.
/// The `events` argument is [`EventLog::take_formatted`] output —
/// `None` (nothing fired) leaves the text untouched.
pub fn append_line(base: String, events: Option<String>) -> String {
    match events {
        Some(line) => format!("{base}\n\n{line}"),
        None => base,
    }
}

/// Spawn the per-page listener tasks that feed `log` from the five
/// subscribed CDP event streams. One task per stream; each exits when
/// the page (and its connection) drops, because `EventStream` then
/// yields `None`.
///
/// Subscribing via `page.event_listener::<T>()` also ENABLES the
/// relevant CDP domains where needed (chromey sends `Page.enable` /
/// sets auto-attach), so dialogs and downloads actually flow.
pub fn attach_page_listeners(page: &chromiumoxide::Page, log: EventLog) {
    use chromiumoxide::cdp::browser_protocol as bp;
    spawn_stream::<bp::page::EventJavascriptDialogOpening>(page, log.clone());
    spawn_stream::<bp::browser::EventDownloadWillBegin>(page, log.clone());
    spawn_stream::<bp::target::EventTargetCreated>(page, log.clone());
    spawn_stream::<bp::target::EventTargetCrashed>(page, log.clone());
    spawn_stream::<bp::inspector::EventDetached>(page, log);
}

fn spawn_stream<T>(page: &chromiumoxide::Page, log: EventLog)
where
    T: chromiumoxide::cdp::IntoEventKind + Into<CdpEvent> + Clone + Send + Unpin + 'static,
{
    let page = page.clone();
    tokio::spawn(async move {
        let mut stream = match page.event_listener::<T>().await {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!(
                    "browser: event subscribe failed for {}: {e}",
                    std::any::type_name::<T>()
                );
                return;
            }
        };
        while let Some(event) = stream.next().await {
            let cdp: CdpEvent = (*event).clone().into();
            if let Some(summary) = map_cdp_event(&cdp) {
                log.push(summary);
            }
        }
    });
}
