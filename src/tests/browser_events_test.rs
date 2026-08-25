//! #1189 — CDP event ring buffer: bounded retention, drain-once
//! surfacing, event-to-summary mapping, and the `recent_events:` line
//! format appended to browser tool results.

use crate::brain::tools::browser::events::{EventLog, RING_CAPACITY, append_line, map_cdp_event};
use chromiumoxide::cdp::CdpEvent;
use chromiumoxide::cdp::browser_protocol as bp;
use chromiumoxide::cdp::browser_protocol::page::DialogType;

fn dialog(message: &str, kind: DialogType) -> CdpEvent {
    CdpEvent::PageJavascriptDialogOpening(bp::page::EventJavascriptDialogOpening {
        url: "https://example.com".into(),
        frame_id: Default::default(),
        message: message.into(),
        r#type: kind,
        has_browser_handler: false,
        default_prompt: None,
    })
}

#[test]
fn ring_is_bounded_at_capacity() {
    let log = EventLog::default();
    for i in 0..(RING_CAPACITY + 7) {
        log.push(format!("event {i}"));
    }
    assert_eq!(log.len(), RING_CAPACITY);
    // Oldest evicted: first drained entry is event 7, not event 0.
    let drained = log.take();
    assert!(
        drained[0].summary.contains("event 7"),
        "oldest should be evicted"
    );
    assert!(
        drained
            .last()
            .unwrap()
            .summary
            .contains(&format!("event {}", RING_CAPACITY + 6))
    );
}

#[test]
fn drain_surfaces_exactly_once() {
    let log = EventLog::default();
    log.push("js-dialog opened (alert)".into());
    let first = log.take_formatted();
    assert!(first.is_some());
    assert!(first.unwrap().starts_with("recent_events: ["));
    // Second drain: nothing — cleared after reporting (spec).
    assert!(log.take_formatted().is_none());
}

#[test]
fn dialog_event_maps_with_kind_and_truncated_message() {
    let ev = dialog(
        "Are you sure you want to delete every single file?",
        DialogType::Confirm,
    );
    let s = map_cdp_event(&ev).expect("dialog should map");
    assert!(s.contains("js-dialog opened (confirm)"), "kind label: {s}");
    assert!(s.contains('…'), "long message truncated: {s}");
    assert!(
        !s.contains("every single file"),
        "full message must not survive: {s}"
    );
}

#[test]
fn download_event_maps_with_filename() {
    let ev = CdpEvent::BrowserDownloadWillBegin(bp::browser::EventDownloadWillBegin {
        frame_id: Default::default(),
        guid: "g".into(),
        url: "https://example.com/f/invoice.pdf".into(),
        suggested_filename: "invoice.pdf".into(),
    });
    let s = map_cdp_event(&ev).expect("download should map");
    assert_eq!(s, "download started: invoice.pdf");
}

#[test]
fn worker_target_creation_is_ignored_page_target_surfaces() {
    let worker = CdpEvent::TargetTargetCreated(Box::new(bp::target::EventTargetCreated {
        target_info: bp::target::TargetInfo::builder()
            .target_id("00000000000000000000000000000001".to_string())
            .r#type("worker")
            .title("sw")
            .attached(true)
            .can_access_opener(false)
            .url("https://example.com/sw.js")
            .build()
            .expect("builder"),
    }));
    assert!(map_cdp_event(&worker).is_none(), "worker targets are noise");

    let page = CdpEvent::TargetTargetCreated(Box::new(bp::target::EventTargetCreated {
        target_info: bp::target::TargetInfo::builder()
            .target_id("00000000000000000000000000000001".to_string())
            .r#type("page")
            .title("popup")
            .attached(true)
            .can_access_opener(false)
            .url("https://example.com/popup")
            .build()
            .expect("builder"),
    }));
    assert!(map_cdp_event(&page).is_some(), "page popups surface");
}

#[test]
fn append_line_attaches_and_passthrough() {
    assert_eq!(append_line("Clicked: #go".into(), None), "Clicked: #go");
    let with = append_line(
        "Clicked: #go".into(),
        Some("recent_events: [21:14:03 js-dialog opened (alert)]".into()),
    );
    assert!(with.starts_with("Clicked: #go\n\nrecent_events: ["));
}
