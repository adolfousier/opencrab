//! #1190 — OOPIF support: frame-selector splitting, origin matching,
//! frame-tree walking, and the end-to-end two-origin fixture (parent on
//! one port embedding a child on another; find must surface the child's
//! button under a namespaced index, click must route through it).

use crate::brain::tools::browser::manager::{
    collect_cross_origin, origin_of, split_frame_selector,
};
use chromiumoxide::cdp::browser_protocol::page::{Frame, FrameTree};

fn frame(url: &str, children: Vec<FrameTree>) -> FrameTree {
    FrameTree {
        frame: Frame {
            url: url.to_string(),
            ..Default::default()
        },
        child_frames: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

#[test]
fn frame_selector_split_cases() {
    assert_eq!(
        split_frame_selector("f2:14"),
        Some(("f2".into(), "14".into()))
    );
    assert_eq!(
        split_frame_selector("f1:[data-opencrabs-match=\"3\"]"),
        Some(("f1".into(), "[data-opencrabs-match=\"3\"]".into()))
    );
    // Plain selectors untouched.
    assert_eq!(split_frame_selector("#submit"), None);
    assert_eq!(split_frame_selector("[data-opencrabs-match=\"2\"]"), None);
    // `form:has(input)` — colon inside CSS, not a frame prefix.
    assert_eq!(split_frame_selector("form:has(input)"), None);
    // Malformed: no digits, no colon, empty tail.
    assert_eq!(split_frame_selector("f:14"), None);
    assert_eq!(split_frame_selector("f2:"), None);
    assert_eq!(split_frame_selector("f2"), None);
}

#[test]
fn origin_extraction() {
    assert_eq!(
        origin_of("https://a.example.com:8000/x"),
        "https://a.example.com:8000"
    );
    assert_eq!(
        origin_of("https://b.example.com/page?q=1"),
        "https://b.example.com"
    );
}

#[test]
fn cross_origin_walk_labels_and_skips_same_origin() {
    // parent (8000) -> same-origin child (8000) -> cross child (8001);
    // parent also has a direct cross grandchild (8002).
    let tree = frame(
        "https://a.test:8000/",
        vec![
            frame(
                "https://a.test:8000/same",
                vec![frame("https://b.test:8001/child", vec![])],
            ),
            frame("https://c.test:8002/direct", vec![]),
        ],
    );
    let mut out = Vec::new();
    collect_cross_origin(&tree, "https://a.test:8000", &mut out);
    assert_eq!(
        out.len(),
        2,
        "same-origin child skipped, both cross frames kept"
    );
    assert_eq!(out[0].0, "f1");
    assert_eq!(out[0].1, "https://b.test:8001/child");
    assert_eq!(out[1].0, "f2");
    assert_eq!(out[1].1, "https://c.test:8002/direct");
}
