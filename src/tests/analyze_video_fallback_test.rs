//! Tests for the analyze_video tool — mime detection and the
//! frame-extraction fallback wiring.
//!
//! Context: the tool tries native Gemini video understanding first, and on
//! any failure (network, Files-API FAILED, API error) falls back to ffmpeg
//! frame extraction + per-frame Gemini vision. The fallback's heavy lifting
//! is I/O (ffmpeg subprocess + Gemini calls) and isn't unit-tested here;
//! these tests pin the pure mime mapping and a source-level guard that the
//! fallback constants stay wired (they were dead code before the fallback
//! was implemented and produced build warnings).

use crate::brain::tools::analyze_video::detect_video_mime_type;

#[test]
fn detects_common_video_mime_types() {
    assert_eq!(detect_video_mime_type("clip.mp4"), "video/mp4");
    assert_eq!(detect_video_mime_type("clip.m4v"), "video/mp4");
    assert_eq!(detect_video_mime_type("clip.mov"), "video/quicktime");
    assert_eq!(detect_video_mime_type("clip.webm"), "video/webm");
    assert_eq!(detect_video_mime_type("clip.mkv"), "video/x-matroska");
    assert_eq!(detect_video_mime_type("clip.avi"), "video/x-msvideo");
    assert_eq!(detect_video_mime_type("clip.3gp"), "video/3gpp");
    assert_eq!(detect_video_mime_type("clip.flv"), "video/x-flv");
}

#[test]
fn mime_detection_is_case_insensitive() {
    assert_eq!(detect_video_mime_type("CLIP.MP4"), "video/mp4");
    assert_eq!(detect_video_mime_type("Movie.MOV"), "video/quicktime");
}

#[test]
fn unknown_extension_defaults_to_mp4() {
    // Gemini is most permissive with mp4; default there rather than reject.
    assert_eq!(detect_video_mime_type("clip.xyz"), "video/mp4");
    assert_eq!(detect_video_mime_type("noext"), "video/mp4");
}

#[test]
fn full_path_is_handled() {
    assert_eq!(
        detect_video_mime_type("/tmp/opencrabs/downloads/recording.webm"),
        "video/webm"
    );
}

// ── Source-level guard: the fallback constants must stay wired ─────────
//
// FALLBACK_MAX_FRAMES and FALLBACK_FPS were added for the frame-extraction
// fallback but left unused for a while, producing dead-code build warnings.
// This guard fails if the fallback wiring that consumes them is removed, so
// the constants never silently go dead again (and the fallback isn't
// accidentally deleted).

const ANALYZE_VIDEO_SRC: &str = include_str!("../brain/tools/analyze_video.rs");

#[test]
fn fallback_constants_are_used_by_extraction() {
    // Strip line comments so the doc-comment mentions don't count as usage.
    let code: String = ANALYZE_VIDEO_SRC
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        code.contains("FALLBACK_MAX_FRAMES.to_string()"),
        "FALLBACK_MAX_FRAMES must feed ffmpeg's -frames:v cap — if this fails the \
         frame-extraction fallback was removed or the constant went dead again."
    );
    assert!(
        code.contains("fps={FALLBACK_FPS}") || code.contains("FALLBACK_FPS"),
        "FALLBACK_FPS must drive the ffmpeg fps filter and the per-frame timestamps."
    );
    assert!(
        code.contains("frame_extraction_fallback"),
        "the frame-extraction fallback method must exist"
    );
    assert!(
        code.contains("ffmpeg"),
        "the fallback must shell out to ffmpeg for frame extraction"
    );
}

#[test]
fn native_failure_triggers_fallback_not_immediate_error() {
    let code: String = ANALYZE_VIDEO_SRC
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    // The execute path must route a failed native result into the fallback,
    // not return it directly. Pin the control-flow shape so a refactor can't
    // quietly drop the fallback hop.
    assert!(
        code.contains("try_native_video") && code.contains("frame_extraction_fallback"),
        "execute must try native video then fall back on failure"
    );
    assert!(
        code.contains("Ok(result) if result.success => return Ok(result)"),
        "only a successful native result short-circuits; failures must fall through to fallback"
    );
}
