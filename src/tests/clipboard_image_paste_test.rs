//! Pasting an image copied from a browser / WhatsApp / a screenshot: the
//! clipboard holds raw image bytes, not a file path, so bracketed paste can't
//! carry it. `App::looks_like_image` gates what the clipboard-read path will
//! attach. (The OS clipboard read itself shells out per platform and is
//! validated on the box, not here.)

use crate::tui::app::App;

#[test]
fn recognizes_common_raster_formats() {
    // PNG
    assert!(App::looks_like_image(&[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0, 0
    ]));
    // JPEG
    assert!(App::looks_like_image(&[
        0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    ]));
    // GIF
    assert!(App::looks_like_image(b"GIF89a and more bytes"));
    // BMP
    assert!(App::looks_like_image(b"BM followed by header"));
    // WEBP (RIFF....WEBP)
    assert!(App::looks_like_image(b"RIFF\0\0\0\0WEBPVP8 "));
}

#[test]
fn rejects_text_and_truncated_payloads() {
    assert!(!App::looks_like_image(b"just some pasted text"));
    assert!(!App::looks_like_image(b"<html><body>more"));
    assert!(!App::looks_like_image(b"hi")); // too short
    assert!(!App::looks_like_image(&[]));
    // RIFF container that is not WEBP (e.g. a WAV) must not pass.
    assert!(!App::looks_like_image(b"RIFF\0\0\0\0WAVEfmt "));
}
