//! Background-task resume helpers shared across channels (#731).

use crate::channels::bg_resume::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// First n polls miss, then the handle appears (late-connect race shape).
#[tokio::test(start_paused = true)]
async fn wait_ready_delivers_after_late_connect() {
    let polls = Arc::new(AtomicUsize::new(0));
    let p = polls.clone();
    let got = wait_ready(
        move || {
            let n = p.fetch_add(1, Ordering::Relaxed);
            async move { if n >= 3 { Some(7u8) } else { None } }
        },
        "test: late connect",
    )
    .await;
    assert_eq!(got, Some(7));
    assert_eq!(polls.load(Ordering::Relaxed), 4);
}

/// Never ready within the bound → None, bounded poll count.
#[tokio::test(start_paused = true)]
async fn wait_ready_times_out_bounded() {
    let polls = Arc::new(AtomicUsize::new(0));
    let p = polls.clone();
    let got = wait_ready(
        move || {
            p.fetch_add(1, Ordering::Relaxed);
            async { None::<u8> }
        },
        "test: timeout",
    )
    .await;
    assert_eq!(got, None);
    assert_eq!(polls.load(Ordering::Relaxed), READY_WAIT_SECS as usize);
}

/// Already-ready handle → immediate delivery, single poll, no sleep.
#[tokio::test(start_paused = true)]
async fn wait_ready_ready_now() {
    let polls = Arc::new(AtomicUsize::new(0));
    let p = polls.clone();
    let got = wait_ready(
        move || {
            p.fetch_add(1, Ordering::Relaxed);
            async { Some("up".to_string()) }
        },
        "test: ready now",
    )
    .await;
    assert_eq!(got.as_deref(), Some("up"));
    assert_eq!(polls.load(Ordering::Relaxed), 1);
}
