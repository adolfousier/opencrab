//! Finding a drag-dropped path inside a typed message (#1288).
//!
//! `extract_image_paths` had two branches that disagreed about spaces. Case 1
//! unescapes and resolves properly but requires the path to be the WHOLE
//! message, so any trailing prose kills it. Case 2 handles prose but iterates
//! `split_whitespace`, so `Screenshot 2026-09-01 at 18.18.16.png` shattered
//! into four words and the only one carrying the extension, `18.18.16.png`,
//! was tested as a relative path and failed.
//!
//! macOS names every screenshot with spaces, so this was the common case.

use std::path::PathBuf;

use crate::tui::app::App;
use crate::tui::app::dropped_path::{self, Dropped};

/// A real file whose name contains spaces, in a temp dir unique per test.
struct Fixture {
    dir: PathBuf,
    path: String,
}

impl Fixture {
    fn new(tag: &str, name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("oc-drop-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(name);
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").expect("fixture write");
        Self {
            dir,
            path: path.to_string_lossy().to_string(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn test_a_spaced_path_followed_by_prose_still_attaches() {
    let f = Fixture::new("prose", "Screenshot 2026-09-01 at 18.18.16.png");
    let msg = format!("{} can you see this image by any chance", f.path);

    let (clean, atts) = App::extract_image_paths(&msg);
    assert_eq!(
        atts.len(),
        1,
        "#1288: this is the reported case and it attached nothing"
    );
    assert_eq!(atts[0].path, f.path);
    assert!(!atts[0].is_video);
    assert!(
        clean.contains("can you see this image"),
        "the question must survive: {clean:?}"
    );
    assert!(
        !clean.contains(".png"),
        "the path must be consumed, not left as prose: {clean:?}"
    );
}

#[test]
fn test_the_shell_escaped_form_attaches_too() {
    // Terminals escape spaces on drop; the raw string then fails exists().
    let f = Fixture::new("escaped", "My Shot 3.png");
    let msg = format!("{} what is in it", f.path.replace(' ', "\\ "));

    let (_clean, atts) = App::extract_image_paths(&msg);
    assert_eq!(atts.len(), 1, "#1288: escaped drop with prose must attach");
    assert_eq!(atts[0].path, f.path);
}

#[test]
fn test_longest_match_wins_over_a_trailing_fragment() {
    // The failure mode this guards: resolving `File.png` from inside
    // `/dir/My File.png` would attach a different file entirely.
    let f = Fixture::new("longest", "My File.png");
    let msg = format!("look at {} please", f.path);

    let (_clean, atts) = App::extract_image_paths(&msg);
    assert_eq!(atts.len(), 1);
    assert_eq!(
        atts[0].path, f.path,
        "must resolve the full path, not the fragment after the space"
    );
}

#[test]
fn test_a_path_from_another_machine_is_marked_not_forwarded() {
    // Dropping from a laptop into a session running over SSH. The bytes are
    // elsewhere, so nothing can attach — but forwarding it as prose sent the
    // agent hunting through the attachments dir and cost a whole turn.
    let msg = "/Users/someone/Downloads/Screenshot 2026-09-01 at 18.18.16.png \
               can you see this image";

    let (clean, atts) = App::extract_image_paths(msg);
    assert!(atts.is_empty(), "nothing to attach: {atts:?}");
    assert!(
        clean.contains("[attachment unavailable:"),
        "#1288: the message must say the path is unreachable: {clean:?}"
    );
    assert!(
        !clean.contains("Screenshot 2026-09-01 at 18.18.16.png can you"),
        "the raw path must not simply be forwarded as prose: {clean:?}"
    );
    // What the marker SAYS depends on the terminal and whether this is an SSH
    // session (#1289), and asserting a specific wording here would make this
    // test pass or fail on where it runs. The per-tier text is covered in
    // tui_remote_upload_test with an injected environment.
    assert!(
        clean.contains("can you see this image"),
        "the question still stands: {clean:?}"
    );
}

#[test]
fn test_a_space_free_path_and_a_url_still_work() {
    // The paths that already worked must keep working.
    let f = Fixture::new("plain", "nospace.png");
    let (clean, atts) = App::extract_image_paths(&format!("{} describe it", f.path));
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0].path, f.path);
    assert!(clean.contains("describe it"));

    let (_c, atts) = App::extract_image_paths("https://example.com/pic.png what is this");
    assert_eq!(atts.len(), 1, "URLs are still picked up");
    assert_eq!(atts[0].path, "https://example.com/pic.png");
}

#[test]
fn test_prose_alone_is_untouched() {
    let msg = "can you look at the png file I mentioned earlier";
    let (clean, atts) = App::extract_image_paths(msg);
    assert!(atts.is_empty());
    assert_eq!(clean.trim(), msg, "ordinary prose must pass through");
}

#[test]
fn test_a_bare_filename_is_not_treated_as_a_drop() {
    // A bare word resolved against the working directory would attach the
    // wrong file, so only absolute and explicitly-relative forms count.
    assert_eq!(dropped_path::find("look at File.png now", &[".png"]), None);
}

#[test]
fn test_extension_must_end_at_a_word_boundary() {
    assert_eq!(dropped_path::find("/a/b/c.pngfoo bar", &[".png"]), None);
}

#[test]
fn test_video_drops_are_flagged_as_video() {
    let f = Fixture::new("video", "Clip 1.mp4");
    let (_clean, atts) = App::extract_image_paths(&format!("{} summarise it", f.path));
    assert_eq!(atts.len(), 1);
    assert!(atts[0].is_video, "#1288: a dropped video must stay a video");
}

#[test]
fn test_find_reports_the_range_it_consumed() {
    let msg = "/nope/Some File.png tail";
    match dropped_path::find(msg, &[".png"]) {
        Some(Dropped::Elsewhere { start, end, path }) => {
            assert_eq!(&msg[start..end], "/nope/Some File.png");
            assert_eq!(path, "/nope/Some File.png");
        }
        other => panic!("expected an unreachable absolute path, got {other:?}"),
    }
}
