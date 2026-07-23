//! #721: outbound media dedup — an identical file+caption uploaded via
//! `telegram_send` must not land twice back-to-back, but a deliberate re-send
//! after the window still goes through.

use crate::channels::telegram::outbound_dedup::MediaSendDedup;
use std::time::{Duration, Instant};

#[test]
fn first_send_is_fresh_immediate_repeat_is_duplicate() {
    let dedup = MediaSendDedup::default();
    let now = Instant::now();
    let sig = MediaSendDedup::signature("send_document", 42, "/tmp/render.mp4", Some("caption"));

    assert!(dedup.claim(sig.clone(), now), "first send is fresh");
    assert!(!dedup.claim(sig, now), "identical repeat is a duplicate");
}

#[test]
fn different_file_or_caption_is_not_a_duplicate() {
    let dedup = MediaSendDedup::default();
    let now = Instant::now();

    let a = MediaSendDedup::signature("send_document", 42, "/tmp/a.mp4", Some("cap"));
    let diff_file = MediaSendDedup::signature("send_document", 42, "/tmp/b.mp4", Some("cap"));
    let diff_caption = MediaSendDedup::signature("send_document", 42, "/tmp/a.mp4", Some("other"));
    let diff_chat = MediaSendDedup::signature("send_document", 99, "/tmp/a.mp4", Some("cap"));

    assert!(dedup.claim(a, now));
    assert!(dedup.claim(diff_file, now), "a different file is not a dup");
    assert!(
        dedup.claim(diff_caption, now),
        "a different caption is not a dup"
    );
    assert!(dedup.claim(diff_chat, now), "a different chat is not a dup");
}

#[test]
fn resending_after_the_window_is_allowed() {
    let dedup = MediaSendDedup::default();
    let now = Instant::now();
    let sig = MediaSendDedup::signature("send_photo", 7, "/tmp/x.png", None);

    assert!(dedup.claim(sig.clone(), now), "first send is fresh");
    let later = now + MediaSendDedup::TTL + Duration::from_secs(1);
    assert!(
        dedup.claim(sig, later),
        "a deliberate re-send after the window lands"
    );
}
