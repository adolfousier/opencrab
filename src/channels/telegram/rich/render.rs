//! Local Mermaid rendering (feature `local-mermaid`) (#1044).
//!
//! Replaces the mermaid.ink URL path: the diagram is parsed and rendered to
//! SVG by `mermaid-render` (pure Rust, dagre-rs layout — the same Sugiyama
//! algorithm mermaid.js uses), then rasterized to PNG bytes in-process by
//! `resvg`. Deterministic and offline: no network state at render time, so
//! Telegram never fetches a third-party URL (kills the mermaid.ink/
//! Cloudflare flakiness class that surfaced as `RICH_MESSAGE_PHOTO_NO_MEDIA_FOUND`).
//!
//! The produced PNG bytes are uploaded to Telegram via multipart (see the
//! delivery path), not referenced by URL.

use super::ast::MermaidResult;

/// Scale factor for the rasterized PNG. 2× keeps text crispy on Telegram's
/// photo sizes without ballooning upload size; mermaid-render returns
/// layout units at {font_size}, so a fixed integer upscale is enough.
const RASTER_SCALE: u32 = 2;

/// Rasterize a mermaid diagram source to PNG bytes, fully in-process.
///
/// Pipeline: `mermaid_render::parse_mermaid` → `render_diagram` (SVG string
/// + width/height in layout units) → `usvg::Tree::from_str` → `resvg::render`
/// onto a `RASTER_SCALE`× pixmap → `Pixmap::encode_png`.
///
/// Returns `Err` (never panics) when the source doesn't parse, the renderer
/// refuses it, or SVG rasterization fails — caller degrades to a legible
/// failure block. `Err` strings are intentionally human-readable for that
/// block.
pub(crate) fn render_local_png(source: &str) -> Result<Vec<u8>, String> {
    use mermaid_render::{DiagramStyle, EstimatedMeasure, render_diagram};

    // Layout with the crate's character-count measure: deterministic, no
    // system font lookup, and accurate enough for flowchart/sequence layout.
    let mut measure = EstimatedMeasure::default();
    let style = DiagramStyle::default();
    let (svg, w, h) = render_diagram(source, &style, &mut measure)
        .map_err(|e| format!("could not render diagram: {e}"))?;

    if w <= 0.0 || h <= 0.0 {
        return Err("diagram has zero size".into());
    }

    // mermaid-render returns an SVG FRAGMENT, not a document: its
    // renderers build `String::new()` and push only inner shapes
    // (`<rect>`, `<path>`, `<text>`, …) — no `<svg xmlns…>` root is ever
    // emitted (see upstream flowchart.rs/render.rs; error_svg.rs is the
    // sole full-document emitter). usvg requires a well-formed document,
    // so wrap the fragment here; #1238.
    let doc = wrap_svg_document(&svg, w, h);

    let mut opt = usvg::Options::default();
    // usvg needs actual fonts to shape diagram labels; default options use
    // an empty database, which renders text as nothing. Load the system set.
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(&doc, &opt).map_err(|e| format!("bad SVG output: {e}"))?;

    let tree_w = tree.size().width().ceil() as u32;
    let tree_h = tree.size().height().ceil() as u32;
    let pw = (tree_w * RASTER_SCALE).max(1);
    let ph = (tree_h * RASTER_SCALE).max(1);

    let mut pixmap =
        tiny_skia::Pixmap::new(pw, ph).ok_or_else(|| "could not allocate image".to_string())?;
    let scale = tiny_skia::Transform::from_scale(RASTER_SCALE as f32, RASTER_SCALE as f32);
    // resvg >=0.45 takes `&mut PixmapMut<'_>`; `as_mut()` alone yields the
    // guard by value (E0308, CI run 33022807360).
    resvg::render(&tree, scale, &mut pixmap.as_mut());

    pixmap
        .encode_png()
        .map_err(|e| format!("could not encode PNG: {e}"))
}

/// Wrap an SVG fragment in a minimal document root, or pass through input
/// that is already a full document/standalone XML file.
///
/// `mermaid-render` returns inner content only; the document envelope with
/// namespace, size and viewBox is our responsibility. Widths are rounded to
/// integers — layout units are f32 but SVG length attributes accept unitless
/// integers, and sub-pixel canvas edges buy nothing at 2× raster scale.
pub(crate) fn wrap_svg_document(svg: &str, width: f32, height: f32) -> String {
    let t = svg.trim_start();
    if t.starts_with("<?xml") || t.starts_with("<svg") {
        return svg.to_string();
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" \
         height=\"{height:.0}\" viewBox=\"0 0 {width:.0} {height:.0}\">{svg}</svg>"
    )
}

/// Render a diagram to a [`MermaidResult`], the shape the mermaid delivery
/// path already consumes. Mirrors the URL path's `prevalidate` outcome but
/// ends in bytes, not a URL — the "image" is uploaded, not referenced.
pub(crate) fn local_render_result(source: &str) -> MermaidResult {
    match render_local_png(source) {
        Ok(bytes) => {
            tracing::debug!(bytes = bytes.len(), "local mermaid render ok");
            MermaidResult::ImageBytes(bytes)
        }
        Err(note) => {
            tracing::warn!(note = %note, "local mermaid render failed");
            MermaidResult::Failed(note)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "graph TD\n    A[\"i start\"] --> B[\"i end\"]\n";

    #[test]
    fn renders_fixture_to_png_bytes() {
        let png = render_local_png(FIXTURE).expect("fixture should render");
        // PNG signature + IHDR present.
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        assert!(png.len() > 8);
    }

    #[test]
    fn result_image_carries_png_bytes() {
        match local_render_result("graph TD\n    A[\"ok\"] --> B[\"ok2\"]\n") {
            MermaidResult::ImageBytes(bytes) => {
                assert_eq!(&bytes[..4], &[0x89, b'P', b'N', b'G']);
            }
            other => panic!("expected ImageBytes, got {other:?}"),
        }
    }

    #[test]
    fn rejects_garbage_source() {
        let err = render_local_png("not a diagram at all").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn wrap_adds_root_to_fragment() {
        let doc = wrap_svg_document("<g></g>", 640.0, 480.0);
        assert!(doc.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(doc.contains("width=\"640\""));
        assert!(doc.contains("viewBox=\"0 0 640 480\""));
        assert!(doc.ends_with("<g></g></svg>"));
    }

    #[test]
    fn wrap_passes_through_document() {
        let full = "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
        assert_eq!(wrap_svg_document(full, 1.0, 1.0), full);
        let xml = "<?xml version=\"1.0\"?><svg xmlns=\"x\"></svg>";
        assert_eq!(wrap_svg_document(xml, 1.0, 1.0), xml);
    }
}