//! Regression: a Telegram caption — the message text a user types alongside a
//! photo / video / document — must reach the agent.
//!
//! Bug (2026-06): every media path combined caption + marker with
//! `caption.is_empty() || marker.contains("<<IMG:")` (and `<<VID:`). The marker
//! emitted by inject_file_content ALWAYS contains its `<<TAG:` sentinel, so the
//! second clause was always true and the caption was silently dropped — the
//! agent saw only the image/file marker, never the user's accompanying text.

use crate::channels::telegram::handler::prepend_caption;

#[test]
fn caption_is_prepended_to_image_marker() {
    let marker = "[User attached an image.]\n<<IMG:/tmp/photo.jpg>>".to_string();
    let combined = prepend_caption("make this a watercolor painting", marker.clone());
    assert!(
        combined.contains("make this a watercolor painting"),
        "the caption (user's message) must reach the agent, got: {combined}"
    );
    assert!(
        combined.contains("<<IMG:"),
        "the image marker must be preserved"
    );
    assert_eq!(
        combined,
        format!("make this a watercolor painting\n\n{marker}")
    );
}

#[test]
fn caption_is_prepended_to_video_marker() {
    let marker = "<<VID:/tmp/clip.mp4>>".to_string();
    let combined = prepend_caption("what's happening in this clip?", marker);
    assert!(combined.starts_with("what's happening in this clip?"));
    assert!(combined.contains("<<VID:"));
}

#[test]
fn empty_or_whitespace_caption_returns_marker_unchanged() {
    let marker = "<<IMG:/tmp/photo.jpg>>".to_string();
    assert_eq!(prepend_caption("", marker.clone()), marker);
    assert_eq!(prepend_caption("   \n ", marker.clone()), marker);
}
