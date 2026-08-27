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

/// Raster scales tried, sharpest first. Layout dims are read off the usvg
/// tree before any allocation, so each candidate is checked against
/// Telegram's photo limits; the sharpest fitting rung wins
/// ([`fit_photo_scale`]).
const PHOTO_SCALE_LADDER: [f32; 5] = [2.0, 1.25, 0.75, 0.5, 0.25];

/// Telegram's `sendPhoto` rejects rasters exceeding ~10_000 px combined
/// (width + height) or on either side. Keep headroom under both caps — the
/// combined cap binds first on large diagrams (#1238: a 44-node TD stack
/// rasterized to ~27k sum and died with `PHOTO_INVALID_DIMENSIONS` despite
/// rendering perfectly).
const PHOTO_MAX_TOTAL_DIMS: f32 = 9_600.0;

/// Pick the sharpest ladder scale whose rasterized dims stay inside the
/// photo box. Uniform scaling preserves aspect ratio, so extreme-aspect
/// layouts (empirical wall ≳45:1) are NOT curable by shrinking — this
/// lifts the size-class ceiling only; those degrade in the delivery path,
/// which reports the pixels it was refused for.
pub(crate) fn fit_photo_scale(base_w: u32, base_h: u32) -> f32 {
    let fits = |s: f32| -> bool {
        let pw = ((base_w as f32) * s).ceil().max(1.0);
        let ph = ((base_h as f32) * s).ceil().max(1.0);
        pw + ph <= PHOTO_MAX_TOTAL_DIMS
    };
    PHOTO_SCALE_LADDER
        .iter()
        .copied()
        .find(|&s| fits(s))
        .unwrap_or(*PHOTO_SCALE_LADDER.last().expect("ladder non-empty"))
}

/// Rasterize a mermaid diagram source to PNG bytes, fully in-process.
///
/// Pipeline: `mermaid_render::parse_mermaid` → `render_diagram` (SVG string
/// + width/height in layout units) → `usvg::Tree::from_str` → `resvg::render`
/// onto an adaptive-scale pixmap (`fit_photo_scale`) → `Pixmap::encode_png`.
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
    // Photo-box-aware raster scale: crisp 2x when it fits, stepping down
    // until the combined dimensions clear Telegram's rejection wall.
    let raster_scale = fit_photo_scale(tree_w, tree_h);
    let pw = ((tree_w as f32) * raster_scale).ceil().max(1.0) as u32;
    let ph = ((tree_h as f32) * raster_scale).ceil().max(1.0) as u32;

    let mut pixmap =
        tiny_skia::Pixmap::new(pw, ph).ok_or_else(|| "could not allocate image".to_string())?;
    let scale = tiny_skia::Transform::from_scale(raster_scale, raster_scale);
    // resvg >=0.45 takes `&mut PixmapMut<'_>`; `as_mut()` alone yields the
    // guard by value (E0308, CI run 33022807360).
    resvg::render(&tree, scale, &mut pixmap.as_mut());

    let png = pixmap
        .encode_png()
        .map_err(|e| format!("could not encode PNG: {e}"))?;
    tracing::debug!(
        bytes = png.len(),
        px_width = pw,
        px_height = ph,
        raster_scale,
        "local mermaid render ok"
    );
    Ok(png)
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
/// path already consumes. Serves as the AUTOMATIC fallback when the
/// mermaid.ink prevalidation fails (see `local_fallback` in the `mermaid`
/// module); ends in bytes, not a URL — the "image" is uploaded, not
/// referenced.
pub(crate) fn local_render_result(source: &str) -> MermaidResult {
    match render_local_png(source) {
        // Success telemetry lives inside `render_local_png`, where pixel
        // geometry and chosen scale are still in scope.
        Ok(bytes) => MermaidResult::ImageBytes(bytes),
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

    #[test]
    fn fit_keeps_sharpest_scale_for_small_diagrams() {
        assert_eq!(fit_photo_scale(640, 480), 2.0);
    }

    #[test]
    fn fit_steps_down_until_tall_stack_fits() {
        // The #1238 size-class: at 2x this is a ~27k-sum reject
        // (PHOTO_INVALID_DIMENSIONS); only the floor rung clears 9_600.
        assert_eq!(fit_photo_scale(1200, 26_000), 0.25);
    }

    #[test]
    fn fit_falls_back_to_floor_when_no_rung_fits() {
        // Nothing shrinks a 40k+40k layout into the box; best effort wins.
        assert_eq!(fit_photo_scale(40_000, 40_000), 0.25);
    }
}
