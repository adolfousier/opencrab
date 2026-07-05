//! Native PDF backend for `generate_document` (#357).
//!
//! Pure writer: the same block specs as the DOCX backend go in, an A4 PDF
//! comes out via `printpdf` using built-in PDF fonts (Helvetica), so no
//! font files or host dependencies are needed. Layout is a simple top-down
//! flow with word wrapping and automatic page breaks. Tables allocate
//! column widths proportionally to their content, draw a separator under
//! the header row plus light rules between rows, and get breathing room
//! around them so they read as actual tables.
//!
//! Built-in PDF fonts are WinAnsi-encoded and printpdf writes text bytes
//! through unre-encoded, so any non-ASCII character renders as mojibake
//! ("â€™" instead of an apostrophe). All text is therefore transliterated
//! to ASCII up front: common punctuation maps to close equivalents, known
//! status emoji map to readable tags, anything else becomes '?'.

use super::docx::BlockSpec;
use printpdf::{
    BuiltinFont, Color, LinePoint, Mm, Op, PaintMode, PdfDocument, PdfFontHandle, PdfPage,
    PdfSaveOptions, Point, Polygon, PolygonRing, Pt, RawImage, Rgb, TextItem, WindingOrder,
    XObjectId, XObjectTransform,
};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

const PAGE_W_MM: f32 = 210.0;
const PAGE_H_MM: f32 = 297.0;
const MARGIN_MM: f32 = 20.0;
/// 1 pt = 0.352778 mm.
const PT_TO_MM: f32 = 0.352_778;
/// Average Helvetica glyph width as a fraction of the font size. Slightly
/// conservative so estimated lines err on wrapping early, never overflowing
/// the right margin.
const AVG_CHAR_WIDTH: f32 = 0.55;
/// Bold glyphs run wider; scale estimates up so bold text wraps earlier.
const BOLD_WIDTH_FACTOR: f32 = 1.08;
/// No table column may claim more than this share of the body width, so one
/// verbose column cannot starve the rest.
const MAX_COL_SHARE: f32 = 0.45;
/// Every table column keeps at least this width (mm), so a column never
/// collapses below a readable sliver.
const MIN_COL_MM: f32 = 8.0;

/// Optional visual styling for a generated PDF (#362). Everything defaults
/// to the plain look, so documents without a style render exactly as before.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct StyleSpec {
    /// Hex color ("#0A84FF") for headings, the H1 underline bar, and the
    /// table header separator.
    pub accent_color: Option<String>,
    /// Hex color for body text (default near-black).
    pub text_color: Option<String>,
    /// Band rendered at the top of every page.
    pub page_header: Option<PageHeaderSpec>,
    /// Line rendered at the bottom of every page.
    pub page_footer: Option<PageFooterSpec>,
    /// Alternating light fills behind table data rows.
    #[serde(default)]
    pub zebra_rows: bool,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PageHeaderSpec {
    pub text: Option<String>,
    /// Local PNG/JPEG path, scaled into the header band (#362, second stage).
    pub logo_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PageFooterSpec {
    pub text: Option<String>,
    #[serde(default)]
    pub page_numbers: bool,
}

/// Parse "#RRGGBB" (with or without '#') into 0..1 RGB.
fn parse_hex(hex: &str) -> Option<(f32, f32, f32)> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((
        byte(0)? as f32 / 255.0,
        byte(2)? as f32 / 255.0,
        byte(4)? as f32 / 255.0,
    ))
}

/// Resolved colors for emission.
struct Palette {
    accent: (f32, f32, f32),
    text: (f32, f32, f32),
    /// True when the user actually set an accent (enables accent-only
    /// decorations like the H1 underline bar).
    accent_set: bool,
}

impl Palette {
    fn from_style(style: &StyleSpec) -> Self {
        let accent = style.accent_color.as_deref().and_then(parse_hex);
        let text = style
            .text_color
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or((0.1, 0.1, 0.1));
        Self {
            accent: accent.unwrap_or((0.25, 0.25, 0.25)),
            text,
            accent_set: accent.is_some(),
        }
    }
}

fn rgb(c: (f32, f32, f32)) -> Color {
    Color::Rgb(Rgb {
        r: c.0,
        g: c.1,
        b: c.2,
        icc_profile: None,
    })
}

/// One laid-out text line ready for emission. Fields are crate-visible so
/// the layout invariants (every wrapped line gets its own baseline) are
/// testable without parsing the serialized PDF back.
pub(crate) struct TextLine {
    pub(crate) text: String,
    pub(crate) bold: bool,
    pub(crate) size: f32,
    pub(crate) indent_mm: f32,
    /// Extra vertical gap (mm) before this line. Negative means "render on
    /// the SAME baseline as the previous line" (table row siblings).
    pub(crate) gap_before_mm: f32,
    /// Rendered in the accent color (headings).
    pub(crate) accent: bool,
}

/// Visual weight/color class of a horizontal rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuleStyle {
    /// Dark (or accent-colored) separator under a table header row.
    HeaderSep,
    /// Light gray rule between table data rows.
    RowLight,
    /// Accent-colored bar (H1 underline). Only emitted when an accent
    /// color is configured.
    Accent,
}

/// One item of the laid-out document flow.
pub(crate) enum LayoutItem {
    Text(TextLine),
    /// Horizontal rule from `x0_mm` to `x1_mm`, drawn `gap_below_baseline_mm`
    /// under the previous baseline.
    Rule {
        x0_mm: f32,
        x1_mm: f32,
        gap_below_baseline_mm: f32,
        style: RuleStyle,
    },
    /// Light fill behind the UPCOMING `height_mm` of content (zebra table
    /// rows). Drawn before the row's text so the text prints on top.
    RowBg {
        height_mm: f32,
    },
    /// Plain vertical whitespace (mm).
    Gap(f32),
}

/// Greedy word wrap by estimated glyph width. `width_mm` is the available
/// horizontal space for the text itself.
fn wrap(text: &str, size_pt: f32, width_mm: f32) -> Vec<String> {
    let max_chars = ((width_mm / PT_TO_MM) / (size_pt * AVG_CHAR_WIDTH)).max(8.0) as usize;
    let mut lines = Vec::new();
    for raw_line in text.split('\n') {
        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            // Hard-split words longer than a full line (paths, URLs) so
            // they wrap instead of running over the column/page edge.
            let mut word = word;
            while word.chars().count() > max_chars {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                let split_at = word
                    .char_indices()
                    .nth(max_chars)
                    .map(|(i, _)| i)
                    .unwrap_or(word.len());
                lines.push(word[..split_at].to_string());
                word = &word[split_at..];
            }
            let candidate_len = if current.is_empty() {
                word.chars().count()
            } else {
                current.chars().count() + 1 + word.chars().count()
            };
            if candidate_len > max_chars && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        lines.push(current);
    }
    lines
}

fn cell_text(cell: &Value) -> String {
    match cell {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Transliterate to ASCII so builtin WinAnsi fonts render it correctly.
/// Never drops content silently: unmappable characters become '?'.
fn ascii_safe(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            _ if c.is_ascii() => out.push(c),
            '\u{2014}' | '\u{2013}' | '\u{2212}' => out.push('-'),
            '\u{2018}' | '\u{2019}' => out.push('\''),
            '\u{201c}' | '\u{201d}' => out.push('"'),
            '\u{2026}' => out.push_str("..."),
            '\u{2022}' | '\u{00b7}' => out.push('-'),
            '\u{00a0}' => out.push(' '),
            '\u{2705}' | '\u{2714}' | '\u{2713}' => out.push_str("[OK]"),
            '\u{274c}' | '\u{2716}' => out.push_str("[X]"),
            '\u{26a0}' => out.push_str("[!]"),
            '\u{2192}' => out.push_str("->"),
            '\u{2190}' => out.push_str("<-"),
            // Zero-width / variation selectors: drop, they are formatting
            // artifacts of emoji sequences, not content.
            '\u{fe0f}' | '\u{200d}' | '\u{200b}' => {}
            _ => out.push('?'),
        }
    }
    out
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 20.0,
        2 => 16.0,
        _ => 13.0,
    }
}

/// Estimated rendered width (mm) of `text` at `size_pt`, bold-adjusted.
fn est_width_mm(text: &str, size_pt: f32, bold: bool) -> f32 {
    let factor = if bold { BOLD_WIDTH_FACTOR } else { 1.0 };
    text.chars().count() as f32 * size_pt * AVG_CHAR_WIDTH * factor * PT_TO_MM
}

/// Content-proportional column widths for a table, fitted to `body_width`.
/// Each column starts from its longest cell's estimated width (bounded by
/// its longest single word, since cells wrap), then gets floored, capped at
/// [`MAX_COL_SHARE`], and scaled to fill the body exactly. A `#` column of
/// single digits stays narrow; a verbose Verdict column gets room without
/// starving the rest.
fn column_widths(rows: &[Vec<Value>], cols: usize, body_width: f32, header_bold: bool) -> Vec<f32> {
    let mut natural = vec![MIN_COL_MM; cols];
    for (r, row) in rows.iter().enumerate() {
        let bold = header_bold && r == 0;
        for (c, cell) in row.iter().enumerate() {
            let text = ascii_safe(&cell_text(cell));
            let longest_word = text
                .split_whitespace()
                .map(|w| est_width_mm(w, 10.0, bold))
                .fold(0.0f32, f32::max);
            let full = est_width_mm(&text, 10.0, bold);
            let want = longest_word.max(full.min(60.0)) + 3.0;
            if want > natural[c] {
                natural[c] = want;
            }
        }
    }
    // Scale to fill the body width, re-clamping to the share cap each pass:
    // scaling up can push a column past the cap, so a single scale+clamp is
    // not enough. Three passes converge close enough; if many columns stay
    // capped the table simply ends narrower than the body, which reads fine.
    let cap = body_width * MAX_COL_SHARE;
    for _ in 0..3 {
        let sum: f32 = natural.iter().sum();
        if sum <= 0.0 {
            break;
        }
        let scale = body_width / sum;
        for w in natural.iter_mut() {
            *w = (*w * scale).clamp(MIN_COL_MM, cap);
        }
    }
    natural
}

/// Flatten blocks into positioned layout items (pure layout, no PDF objects
/// yet).
pub(crate) fn layout(blocks: &[BlockSpec], style: &StyleSpec) -> Vec<LayoutItem> {
    let accent_set = style.accent_color.as_deref().and_then(parse_hex).is_some();
    let body_width = PAGE_W_MM - 2.0 * MARGIN_MM;
    let mut items: Vec<LayoutItem> = Vec::new();
    for block in blocks {
        match block {
            BlockSpec::Heading { text, level } => {
                let size = heading_size((*level).clamp(1, 3));
                for (i, l) in wrap(&ascii_safe(text), size, body_width)
                    .into_iter()
                    .enumerate()
                {
                    items.push(LayoutItem::Text(TextLine {
                        text: l,
                        bold: true,
                        size,
                        indent_mm: 0.0,
                        gap_before_mm: if i == 0 { 4.0 } else { 0.0 },
                        accent: true,
                    }));
                }
                // Accent underline bar below H1 headings when branded.
                if accent_set && (*level).clamp(1, 3) == 1 {
                    items.push(LayoutItem::Rule {
                        x0_mm: MARGIN_MM,
                        x1_mm: MARGIN_MM + 26.0,
                        gap_below_baseline_mm: 1.6,
                        style: RuleStyle::Accent,
                    });
                    items.push(LayoutItem::Gap(1.2));
                }
            }
            BlockSpec::Paragraph { text, bold } => {
                for (i, l) in wrap(&ascii_safe(text), 11.0, body_width)
                    .into_iter()
                    .enumerate()
                {
                    items.push(LayoutItem::Text(TextLine {
                        text: l,
                        bold: *bold,
                        size: 11.0,
                        indent_mm: 0.0,
                        gap_before_mm: if i == 0 { 2.0 } else { 0.0 },
                        accent: false,
                    }));
                }
            }
            BlockSpec::List {
                items: list_items,
                ordered,
            } => {
                for (n, item) in list_items.iter().enumerate() {
                    let marker = if *ordered {
                        format!("{}. ", n + 1)
                    } else {
                        "- ".to_string()
                    };
                    for (i, l) in wrap(&ascii_safe(item), 11.0, body_width - 6.0)
                        .into_iter()
                        .enumerate()
                    {
                        items.push(LayoutItem::Text(TextLine {
                            text: if i == 0 { format!("{marker}{l}") } else { l },
                            bold: false,
                            size: 11.0,
                            indent_mm: if i == 0 { 4.0 } else { 8.0 },
                            gap_before_mm: if i == 0 { 1.0 } else { 0.0 },
                            accent: false,
                        }));
                    }
                }
            }
            BlockSpec::Table { rows, header_bold } => {
                let cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
                let widths = column_widths(rows, cols, body_width, *header_bold);
                let mut offsets = vec![0.0f32; cols];
                for c in 1..cols {
                    offsets[c] = offsets[c - 1] + widths[c - 1];
                }
                items.push(LayoutItem::Gap(2.5));
                for (r, row) in rows.iter().enumerate() {
                    let is_header = *header_bold && r == 0;
                    // Light rule between data rows (not above the first data
                    // row, which already sits under the header separator).
                    if r > 0 && !(*header_bold && r == 1) {
                        items.push(LayoutItem::Rule {
                            x0_mm: MARGIN_MM,
                            x1_mm: MARGIN_MM + body_width,
                            gap_below_baseline_mm: 1.4,
                            style: RuleStyle::RowLight,
                        });
                    }
                    let wrapped: Vec<Vec<String>> = row
                        .iter()
                        .enumerate()
                        .map(|(c, cell)| wrap(&ascii_safe(&cell_text(cell)), 10.0, widths[c] - 2.5))
                        .collect();
                    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
                    // Zebra fill behind every second data row.
                    let data_row_idx = if *header_bold { r.wrapping_sub(1) } else { r };
                    if style.zebra_rows && !is_header && data_row_idx % 2 == 1 {
                        let row_h = height as f32 * 10.0 * PT_TO_MM * 1.35 + 0.8;
                        items.push(LayoutItem::RowBg { height_mm: row_h });
                    }
                    for line_idx in 0..height {
                        // The FIRST emitted cell of each visual row-line
                        // advances the baseline; the rest share it. Keying
                        // the advance on column 0 skipped the advance
                        // whenever column 0 had fewer wrapped lines than a
                        // sibling, stacking later cells onto the previous
                        // line (visible as overlapping table text).
                        let mut advanced = false;
                        for (c, cell_lines) in wrapped.iter().enumerate() {
                            if let Some(text) = cell_lines.get(line_idx)
                                && !text.is_empty()
                            {
                                let row_gap = if line_idx > 0 || r == 0 { 0.0 } else { 0.8 };
                                items.push(LayoutItem::Text(TextLine {
                                    text: text.clone(),
                                    bold: is_header,
                                    size: 10.0,
                                    indent_mm: offsets[c],
                                    gap_before_mm: if advanced { -1.0 } else { row_gap },
                                    accent: false,
                                }));
                                advanced = true;
                            }
                        }
                    }
                    // Separator under the header row, with breathing room.
                    if is_header {
                        items.push(LayoutItem::Rule {
                            x0_mm: MARGIN_MM,
                            x1_mm: MARGIN_MM + body_width,
                            gap_below_baseline_mm: 1.6,
                            style: RuleStyle::HeaderSep,
                        });
                        items.push(LayoutItem::Gap(0.8));
                    }
                }
                items.push(LayoutItem::Gap(3.0));
            }
        }
    }
    items
}

/// Ops drawing a horizontal rule at `y_mm`.
fn rule_ops(
    x0_mm: f32,
    x1_mm: f32,
    y_mm: f32,
    color: (f32, f32, f32),
    thickness_pt: f32,
) -> Vec<Op> {
    vec![
        Op::SetOutlineColor { col: rgb(color) },
        Op::SetOutlineThickness {
            pt: Pt(thickness_pt),
        },
        Op::DrawLine {
            line: printpdf::Line {
                points: vec![
                    LinePoint {
                        p: Point::new(Mm(x0_mm), Mm(y_mm)),
                        bezier: false,
                    },
                    LinePoint {
                        p: Point::new(Mm(x1_mm), Mm(y_mm)),
                        bezier: false,
                    },
                ],
                is_closed: false,
            },
        },
    ]
}

/// Filled light-gray rectangle ops (zebra row background).
fn fill_rect_ops(x0: f32, x1: f32, y_top: f32, y_bottom: f32) -> Vec<Op> {
    let pts = [(x0, y_top), (x1, y_top), (x1, y_bottom), (x0, y_bottom)];
    vec![
        Op::SetFillColor {
            col: rgb((0.94, 0.94, 0.94)),
        },
        Op::DrawPolygon {
            polygon: Polygon {
                rings: vec![PolygonRing {
                    points: pts
                        .iter()
                        .map(|(x, y)| LinePoint {
                            p: Point::new(Mm(*x), Mm(*y)),
                            bezier: false,
                        })
                        .collect(),
                }],
                mode: PaintMode::Fill,
                winding_order: WindingOrder::NonZero,
            },
        },
    ]
}

/// A page-header logo registered with the document.
struct Logo {
    id: XObjectId,
    /// dpi that renders the image at the target header height.
    dpi: f32,
    /// Rendered width (mm) at that dpi, for placing header text beside it.
    width_mm: f32,
}

/// Target rendered height of a header logo (mm).
const LOGO_H_MM: f32 = 8.0;

/// Load and register the header logo, if configured. A missing or
/// undecodable image logs a warning and the PDF renders without it; a bad
/// logo must never fail the whole document.
fn load_logo(style: &StyleSpec, doc: &mut PdfDocument) -> Option<Logo> {
    let path = style.page_header.as_ref()?.logo_path.as_deref()?;
    let expanded = crate::brain::tools::error::expand_tilde(path);
    let bytes = match std::fs::read(&expanded) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("generate_document: logo not readable at {path}: {e} — skipping logo");
            return None;
        }
    };
    let mut warnings = Vec::new();
    let image = match RawImage::decode_from_bytes(&bytes, &mut warnings) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!("generate_document: logo at {path} failed to decode: {e} — skipping");
            return None;
        }
    };
    let logo_h_pt = LOGO_H_MM / PT_TO_MM;
    // dpi maps pixels to points: rendered_pt = px * 72 / dpi.
    let dpi = image.height as f32 * 72.0 / logo_h_pt;
    let width_mm = image.width as f32 * 72.0 / dpi * PT_TO_MM;
    let id = doc.add_image(&image);
    Some(Logo { id, dpi, width_mm })
}

/// Header/footer furniture ops for one page.
fn furniture_ops(
    style: &StyleSpec,
    palette: &Palette,
    logo: Option<&Logo>,
    page_no: usize,
    page_total: usize,
) -> Vec<Op> {
    let mut ops = Vec::new();
    let has_header_content = style
        .page_header
        .as_ref()
        .is_some_and(|h| h.text.is_some() || h.logo_path.is_some());
    if has_header_content {
        let y = PAGE_H_MM - 12.0;
        let mut text_x = MARGIN_MM;
        if let Some(logo) = logo {
            // Bottom-align the logo band with the header text baseline.
            ops.push(Op::UseXobject {
                id: logo.id.clone(),
                transform: XObjectTransform {
                    translate_x: Some(Pt(MARGIN_MM / PT_TO_MM)),
                    translate_y: Some(Pt((y - 2.0) / PT_TO_MM)),
                    dpi: Some(logo.dpi),
                    ..Default::default()
                },
            });
            text_x += logo.width_mm + 3.0;
        }
        ops.extend(rule_ops(
            MARGIN_MM,
            PAGE_W_MM - MARGIN_MM,
            y - 2.0,
            palette.accent,
            0.8,
        ));
        if let Some(text) = style.page_header.as_ref().and_then(|h| h.text.as_deref()) {
            ops.extend([
                Op::StartTextSection,
                Op::SetTextCursor {
                    pos: Point::new(Mm(text_x), Mm(y)),
                },
                Op::SetFont {
                    font: PdfFontHandle::Builtin(BuiltinFont::HelveticaBold),
                    size: Pt(9.0),
                },
                Op::SetFillColor {
                    col: rgb(palette.accent),
                },
                Op::ShowText {
                    items: vec![TextItem::Text(ascii_safe(text))],
                },
                Op::EndTextSection,
            ]);
        }
    }
    if let Some(footer) = &style.page_footer {
        let y = 11.0;
        ops.extend(rule_ops(
            MARGIN_MM,
            PAGE_W_MM - MARGIN_MM,
            y + 4.0,
            (0.75, 0.75, 0.75),
            0.4,
        ));
        if let Some(text) = footer.text.as_deref() {
            ops.extend([
                Op::StartTextSection,
                Op::SetTextCursor {
                    pos: Point::new(Mm(MARGIN_MM), Mm(y)),
                },
                Op::SetFont {
                    font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                    size: Pt(8.0),
                },
                Op::SetFillColor {
                    col: rgb((0.45, 0.45, 0.45)),
                },
                Op::ShowText {
                    items: vec![TextItem::Text(ascii_safe(text))],
                },
                Op::EndTextSection,
            ]);
        }
        if footer.page_numbers {
            let label = format!("Page {page_no} of {page_total}");
            let x = PAGE_W_MM - MARGIN_MM - est_width_mm(&label, 8.0, false);
            ops.extend([
                Op::StartTextSection,
                Op::SetTextCursor {
                    pos: Point::new(Mm(x), Mm(y)),
                },
                Op::SetFont {
                    font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                    size: Pt(8.0),
                },
                Op::SetFillColor {
                    col: rgb((0.45, 0.45, 0.45)),
                },
                Op::ShowText {
                    items: vec![TextItem::Text(label)],
                },
                Op::EndTextSection,
            ]);
        }
    }
    ops
}

/// Write `blocks` to a PDF at `path`. Returns a short human summary.
pub(crate) fn write_pdf(
    path: &Path,
    blocks: &[BlockSpec],
    title: &str,
    style: &StyleSpec,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let palette = Palette::from_style(style);
    // Reserve room for page furniture when configured.
    let has_header = style
        .page_header
        .as_ref()
        .is_some_and(|h| h.text.is_some() || h.logo_path.is_some());
    let has_footer = style.page_footer.is_some();
    let content_top = if has_header {
        PAGE_H_MM - MARGIN_MM - 6.0
    } else {
        PAGE_H_MM - MARGIN_MM
    };
    let content_bottom = if has_footer {
        MARGIN_MM - 2.0 + 8.0
    } else {
        MARGIN_MM
    };

    let items = layout(blocks, style);
    let mut page_ops: Vec<Vec<Op>> = Vec::new();
    let mut ops: Vec<Op> = Vec::new();
    let mut y_mm = content_top;
    let mut text_lines = 0usize;

    for item in &items {
        match item {
            LayoutItem::Gap(mm) => {
                y_mm -= mm;
            }
            LayoutItem::Rule {
                x0_mm,
                x1_mm,
                gap_below_baseline_mm,
                style: rule_style,
            } => {
                let rule_y = y_mm - gap_below_baseline_mm;
                // Rules never force a page break; if the page is full the
                // next text line breaks the page and the rule is skipped.
                if rule_y > content_bottom {
                    let (color, thickness) = match rule_style {
                        RuleStyle::HeaderSep => {
                            let c = if palette.accent_set {
                                palette.accent
                            } else {
                                (0.25, 0.25, 0.25)
                            };
                            (c, 0.8)
                        }
                        RuleStyle::RowLight => ((0.75, 0.75, 0.75), 0.4),
                        RuleStyle::Accent => (palette.accent, 1.2),
                    };
                    ops.extend(rule_ops(*x0_mm, *x1_mm, rule_y, color, thickness));
                }
            }
            LayoutItem::RowBg { height_mm } => {
                // Painted from just under the previous baseline down over the
                // upcoming row. Skipped when the row will page-break (the
                // fill would land on the old page); imperfect but harmless.
                let top = y_mm - 1.0;
                let bottom = top - height_mm;
                if bottom > content_bottom {
                    ops.extend(fill_rect_ops(
                        MARGIN_MM - 1.0,
                        PAGE_W_MM - MARGIN_MM + 1.0,
                        top,
                        bottom,
                    ));
                }
            }
            LayoutItem::Text(line) => {
                text_lines += 1;
                let line_height_mm = line.size * PT_TO_MM * 1.35;
                let same_baseline = line.gap_before_mm < 0.0;
                if !same_baseline {
                    y_mm -= line.gap_before_mm + line_height_mm;
                }
                if y_mm < content_bottom {
                    page_ops.push(std::mem::take(&mut ops));
                    y_mm = content_top - line_height_mm;
                }
                let font = if line.bold {
                    BuiltinFont::HelveticaBold
                } else {
                    BuiltinFont::Helvetica
                };
                let color = if line.accent && palette.accent_set {
                    palette.accent
                } else {
                    palette.text
                };
                ops.extend([
                    Op::StartTextSection,
                    Op::SetTextCursor {
                        pos: Point::new(Mm(MARGIN_MM + line.indent_mm), Mm(y_mm)),
                    },
                    Op::SetFont {
                        font: PdfFontHandle::Builtin(font),
                        size: Pt(line.size),
                    },
                    Op::SetFillColor { col: rgb(color) },
                    Op::ShowText {
                        items: vec![TextItem::Text(line.text.clone())],
                    },
                    Op::EndTextSection,
                ]);
            }
        }
    }
    page_ops.push(ops);

    let mut doc = PdfDocument::new(title);
    let logo = load_logo(style, &mut doc);
    let page_total = page_ops.len();
    let pages: Vec<PdfPage> = page_ops
        .into_iter()
        .enumerate()
        .map(|(i, mut content)| {
            let mut all = furniture_ops(style, &palette, logo.as_ref(), i + 1, page_total);
            all.append(&mut content);
            PdfPage::new(Mm(PAGE_W_MM), Mm(PAGE_H_MM), all)
        })
        .collect();

    let mut warnings = Vec::new();
    let bytes = doc
        .with_pages(pages)
        .save(&PdfSaveOptions::default(), &mut warnings);
    if !warnings.is_empty() {
        tracing::warn!(
            "generate_document: printpdf reported {} warning(s) while saving {}",
            warnings.len(),
            path.display()
        );
    }
    std::fs::write(path, bytes)?;
    Ok(format!("{} page(s), {} line(s)", page_total, text_lines))
}
