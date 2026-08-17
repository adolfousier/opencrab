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

/// One slide: a title, optional bullet lines, optional speaker notes, and
/// an optional slide-layout index (into the template's layout list).
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct SlideSpec {
    pub title: String,
    #[serde(default)]
    pub bullets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<usize>,
}

/// Optional deck styling (#366). Defaults keep the plain look.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct PptxStyle {
    /// Existing .pptx used as the template: slides inherit its master
    /// (logos, fonts, backgrounds). The highest-leverage branding lever.
    pub template_path: Option<String>,
    /// Hex color ("#0A84FF") applied to slide titles when no template
    /// carries brand styling.
    pub accent_color: Option<String>,
    /// Slide dimensions (#441): a preset string ("16:9", "4:3", "square")
    /// or an object {width_inches, height_inches}. Ignored when
    /// template_path is set (template dims inherited).
    pub slide_size: Option<serde_json::Value>,
}

/// Validate "#RRGGBB"; invalid colors are dropped (with a log) so a typo
/// degrades to the plain look instead of failing deck generation.
fn valid_hex(hex: &str) -> Option<String> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("#{h}"))
    } else {
        tracing::warn!("generate_document: ignoring invalid pptx accent color {hex:?}");
        None
    }
}

/// Fixed generator script. Reads `{"slides": [...]}` JSON from stdin,
/// writes the deck to the path in argv[1]. Only ever edited here; content
/// never gets formatted into it.
const PPTX_SCRIPT: &str = r##"
import json, sys
from pptx import Presentation
from pptx.dml.color import RGBColor

spec = json.load(sys.stdin)
tpl = spec.get("template")
prs = Presentation(tpl) if tpl else Presentation()
slide_size = spec.get("slide_size")
if slide_size and not tpl:
    _PRESETS = {
        "16:9": (12192000, 6858000),
        "4:3": (9144000, 6858000),
        "square": (9144000, 9144000),
    }
    if isinstance(slide_size, str):
        w, h = _PRESETS.get(slide_size, (12192000, 6858000))
    elif isinstance(slide_size, dict):
        w = int(slide_size.get("width_inches", 13.33) * 914400)
        h = int(slide_size.get("height_inches", 7.5) * 914400)
    else:
        w, h = 12192000, 6858000
    prs.slide_width = w
    prs.slide_height = h
accent = spec.get("accent")

def hex_rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))

for s in spec["slides"]:
    idx = s.get("layout")
    if idx is None or idx >= len(prs.slide_layouts):
        idx = 1 if len(prs.slide_layouts) > 1 else 0
    slide = prs.slides.add_slide(prs.slide_layouts[idx])
    if slide.shapes.title is not None:
        slide.shapes.title.text = s.get("title", "")
        if accent:
            r, g, b = hex_rgb(accent)
            for p in slide.shapes.title.text_frame.paragraphs:
                for run in p.runs:
                    run.font.color.rgb = RGBColor(r, g, b)
    bullets = s.get("bullets", [])
    if bullets:
        body = None
        for ph in slide.placeholders:
            if ph.placeholder_format.idx != 0 and ph.has_text_frame:
                body = ph
                break
        if body is not None:
            tf = body.text_frame
            tf.text = bullets[0]
            for b in bullets[1:]:
                p = tf.add_paragraph()
                p.text = b
    notes = s.get("notes")
    if notes:
        slide.notes_slide.notes_text_frame.text = notes
prs.save(sys.argv[1])
print(f"{len(spec['slides'])} slide(s)")
"##;

/// Guidance appended to every unavailability error so channel users get an
/// actionable message instead of a bare failure.
const INSTALL_HINT: &str = "PPTX generation needs python3 with the python-pptx package on this host \
     (install with: pip3 install python-pptx). XLSX, DOCX, and PDF work without it.";

/// Write `slides` to a .pptx at `path` via the host's python-pptx.
pub(crate) async fn write_deck(
    path: &Path,
    slides: &[SlideSpec],
    style: &PptxStyle,
) -> Result<String, String> {
    let template = match style.template_path.as_deref() {
        Some(t) => {
            let expanded = crate::brain::tools::error::expand_tilde(t);
            if !expanded.exists() {
                return Err(format!(
                    "Template not found at {t}. Fix the template_path or omit it \
                     to generate on the default blank master."
                ));
            }
            Some(expanded.to_string_lossy().into_owned())
        }
        None => None,
    };
    let accent = style.accent_color.as_deref().and_then(valid_hex);
    let payload = serde_json::json!({
        "slides": slides,
        "template": template,
        "accent": accent,
        "slide_size": style.slide_size,
    })
    .to_string();

    let mut child = tokio::process::Command::new("python3")
        .kill_on_drop(true)
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
