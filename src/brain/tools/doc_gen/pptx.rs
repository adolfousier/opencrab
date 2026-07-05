//! PPTX backend for `generate_document` (#357).
//!
//! No mature Rust crate exists for PowerPoint generation, so this backend
//! dispatches to `python-pptx` when the host has it and returns a clear
//! "not available on this host" error otherwise. Slide data travels to the
//! fixed Python script via stdin as JSON and the output path via argv, so
//! user content is never interpolated into code.

use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::AsyncWriteExt;

/// One slide: a title, optional bullet lines, optional speaker notes.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct SlideSpec {
    pub title: String,
    #[serde(default)]
    pub bullets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Fixed generator script. Reads `{"slides": [...]}` JSON from stdin,
/// writes the deck to the path in argv[1]. Only ever edited here; content
/// never gets formatted into it.
const PPTX_SCRIPT: &str = r#"
import json, sys
from pptx import Presentation
from pptx.util import Inches, Pt

spec = json.load(sys.stdin)
prs = Presentation()
layout = prs.slide_layouts[1]  # Title and Content
for s in spec["slides"]:
    slide = prs.slides.add_slide(layout)
    slide.shapes.title.text = s.get("title", "")
    body = slide.placeholders[1].text_frame
    bullets = s.get("bullets", [])
    if bullets:
        body.text = bullets[0]
        for b in bullets[1:]:
            p = body.add_paragraph()
            p.text = b
    notes = s.get("notes")
    if notes:
        slide.notes_slide.notes_text_frame.text = notes
prs.save(sys.argv[1])
print(f"{len(spec['slides'])} slide(s)")
"#;

/// Guidance appended to every unavailability error so channel users get an
/// actionable message instead of a bare failure.
const INSTALL_HINT: &str = "PPTX generation needs python3 with the python-pptx package on this host \
     (install with: pip3 install python-pptx). XLSX, DOCX, and PDF work without it.";

/// Write `slides` to a .pptx at `path` via the host's python-pptx.
pub(crate) async fn write_deck(path: &Path, slides: &[SlideSpec]) -> Result<String, String> {
    let payload = serde_json::json!({ "slides": slides }).to_string();

    let mut child = tokio::process::Command::new("python3")
        .arg("-c")
        .arg(PPTX_SCRIPT)
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("python3 could not be started ({e}). {INSTALL_HINT}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| format!("failed to send slide data to python3: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .await
        .map_err(|e| format!("python3 did not finish: {e}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("No module named") {
            return Err(format!("python-pptx is not installed. {INSTALL_HINT}"));
        }
        return Err(format!(
            "python-pptx generation failed: {}",
            crate::utils::truncate_str(stderr.trim(), 500)
        ));
    }
    let summary = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if summary.is_empty() {
        Ok(format!("{} slide(s)", slides.len()))
    } else {
        Ok(summary)
    }
}
