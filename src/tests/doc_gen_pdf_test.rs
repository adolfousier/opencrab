//! Tests for the `generate_document` PDF backend (#357).
//!
//! Generated PDFs are read back with `pdf-extract`, the same crate the
//! read side uses, so the round trip proves our own tooling can consume
//! what we write.

use crate::brain::tools::doc_gen::docx::BlockSpec;
use crate::brain::tools::doc_gen::pdf::{StyleSpec, write_pdf};
use serde_json::json;

fn blocks(v: serde_json::Value) -> Vec<BlockSpec> {
    serde_json::from_value(v).expect("valid block specs")
}

#[test]
fn pdf_contains_heading_paragraph_list_and_table_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("report.pdf");
    let summary = write_pdf(
        &path,
        &blocks(json!([
            {"type": "heading", "text": "Annual Summary", "level": 1},
            {"type": "paragraph", "text": "All systems operated normally."},
            {"type": "list", "items": ["uptime held", "costs flat"], "ordered": true},
            {"type": "table", "rows": [["Metric", "Value"], ["Requests", 12345]]}
        ])),
        "Annual Summary",
        &StyleSpec::default(),
    )
    .expect("pdf written");
    assert!(summary.contains("1 page(s)"));

    let text = pdf_extract::extract_text(&path).expect("pdf extracts");
    assert!(text.contains("Annual Summary"), "text: {text}");
    assert!(text.contains("All systems operated normally."));
    assert!(text.contains("uptime held"));
    assert!(text.contains("1."));
    assert!(text.contains("Requests"));
    assert!(text.contains("12345"));
}

#[test]
fn long_content_breaks_across_pages() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("long.pdf");
    let many: Vec<serde_json::Value> = (0..120)
        .map(|i| json!({"type": "paragraph", "text": format!("Paragraph number {i} of filler content.")}))
        .collect();
    let summary =
        write_pdf(&path, &blocks(json!(many)), "Long", &StyleSpec::default()).expect("pdf written");
    let pages: usize = summary
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .expect("summary starts with page count");
    assert!(
        pages >= 2,
        "expected multiple pages, got summary: {summary}"
    );

    let text = pdf_extract::extract_text(&path).expect("pdf extracts");
    assert!(text.contains("Paragraph number 0"));
    assert!(text.contains("Paragraph number 119"));
}

#[test]
fn long_paragraph_wraps_instead_of_overflowing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wrap.pdf");
    let long_text = "word ".repeat(200);
    let summary = write_pdf(
        &path,
        &blocks(json!([{"type": "paragraph", "text": long_text}])),
        "Wrap",
        &StyleSpec::default(),
    )
    .expect("pdf written");
    // 200 short words cannot fit one line: the layout must emit many lines.
    let lines: usize = summary
        .split("page(s), ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .expect("summary includes line count");
    assert!(lines > 5, "expected wrapped lines, got summary: {summary}");
}

#[test]
fn non_ascii_transliterates_instead_of_mojibake() {
    // Builtin WinAnsi fonts render raw UTF-8 bytes as garbage ("â€™"), so
    // all text must be transliterated to ASCII before layout.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("ascii.pdf");
    write_pdf(
        &path,
        &blocks(json!([
            {"type": "heading", "text": "Audit — Results ✅"},
            {"type": "paragraph", "text": "It’s “done”… → next 📱"}
        ])),
        "Audit",
        &StyleSpec::default(),
    )
    .expect("pdf written");
    let text = pdf_extract::extract_text(&path).expect("pdf extracts");
    assert!(text.contains("Audit - Results [OK]"), "text: {text}");
    assert!(text.contains("It's \"done\"... -> next ?"), "text: {text}");
    // No raw multibyte sequences leak through.
    assert!(!text.contains('\u{2019}'));
    assert!(!text.contains("â"));
}

#[test]
fn table_with_uneven_cell_heights_keeps_all_text() {
    // Column 0 shorter than column 2 used to skip the baseline advance and
    // stack lines on top of each other; every cell line must still land.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("uneven.pdf");
    write_pdf(
        &path,
        &blocks(json!([
            {"type": "table", "rows": [
                ["Feature", "Status", "Notes"],
                ["A", "ok", "a very long explanation cell that definitely wraps across multiple lines in its column"],
                ["B", "ok", "short"]
            ]}
        ])),
        "Uneven",
        &StyleSpec::default(),
    )
    .expect("pdf written");
    let text = pdf_extract::extract_text(&path).expect("pdf extracts");
    assert!(text.contains("Feature"));
    assert!(text.contains("definitely"));
    assert!(text.contains("wraps"));
    assert!(text.contains("short"));
}

#[test]
fn unbroken_long_words_hard_split() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("longword.pdf");
    let long_path = format!("/very/long/{}/file.rs", "sub/".repeat(40));
    write_pdf(
        &path,
        &blocks(json!([{"type": "paragraph", "text": long_path}])),
        "Longword",
        &StyleSpec::default(),
    )
    .expect("pdf written");
    let text = pdf_extract::extract_text(&path).expect("pdf extracts");
    assert!(text.contains("/very/long/"));
    assert!(text.contains("file.rs"));
}

#[test]
fn audit_table_shape_every_wrapped_line_gets_its_own_baseline() {
    // Regression proof for the overlapping-table garble: reproduce the
    // audit-report shape (4 columns, bold header, cells wrapping to
    // different heights) and simulate the emitter's y accounting. Any two
    // lines in the SAME column must land on DIFFERENT baselines; lines on a
    // shared baseline must be in different columns.
    use crate::brain::tools::doc_gen::pdf::{LayoutItem, layout};

    let specs = blocks(json!([
        {"type": "table", "rows": [
            ["Feature", "Adi says OpenClaw", "OpenCrabs actually", "Verdict"],
            ["Basic send/reply/edit/delete", "Yes", "Yes - handler.rs", "Match"],
            ["Photo/Doc/Location/Poll", "Yes", "Yes - telegram_send.rs", "Match"],
            ["Rich messages (Bot API 10.1)", "Yes - tables, formulas",
             "Yes - rich/api.rs with tables, headings, math", "WRONG - WE HAVE IT"],
            ["Streaming preview + reasoning", "Yes - preview stream",
             "Yes - draft_streaming, ProviderStream", "WRONG - WE HAVE IT"]
        ]}
    ]));
    let items = layout(&specs, &StyleSpec::default());

    // Simulate the emitter: negative gap = same baseline, else advance.
    let mut y = 297.0f32 - 20.0;
    let mut placed: Vec<(f32, f32, String)> = Vec::new(); // (y, x, text)
    for item in &items {
        match item {
            LayoutItem::Gap(mm) => y -= mm,
            LayoutItem::Rule { .. } | LayoutItem::RowBg { .. } => {}
            LayoutItem::Text(l) => {
                let h = l.size * 0.352_778 * 1.35;
                if l.gap_before_mm >= 0.0 {
                    y -= l.gap_before_mm + h;
                }
                placed.push((y, l.indent_mm, l.text.clone()));
            }
        }
    }
    assert!(placed.len() > 10, "table must produce many lines");
    for (i, a) in placed.iter().enumerate() {
        for b in placed.iter().skip(i + 1) {
            if (a.0 - b.0).abs() < 0.01 {
                assert!(
                    (a.1 - b.1).abs() > 0.01,
                    "two lines share baseline y={} AND column x={}: {:?} vs {:?}",
                    a.0,
                    a.1,
                    a.2,
                    b.2
                );
            }
        }
    }
}

#[test]
fn table_columns_size_to_content() {
    // A single-digit "#" column must stay narrow while verbose columns get
    // the freed space; no column may exceed the max share cap.
    use crate::brain::tools::doc_gen::pdf::{LayoutItem, layout};

    let specs = blocks(json!([
        {"type": "table", "rows": [
            ["#", "Feature", "Verdict"],
            ["1", "Multiple bot accounts", "TRUE GAP"],
            ["2", "Rich messages (image+text)", "WRONG: rich/api.rs"],
            ["17", "Slash commands", "WRONG: commands.toml"]
        ]}
    ]));
    let items = layout(&specs, &StyleSpec::default());
    // Collect the distinct x offsets used by table text: these are the
    // column starts. The second column's start is the first column's width.
    let mut xs: Vec<f32> = items
        .iter()
        .filter_map(|i| match i {
            LayoutItem::Text(l) => Some(l.indent_mm),
            _ => None,
        })
        .collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    assert_eq!(xs.len(), 3, "three column starts, got {xs:?}");
    let body = 210.0 - 40.0;
    let col0_width = xs[1] - xs[0];
    assert!(
        col0_width < body * 0.15,
        "digit column must stay narrow, got {col0_width}mm of {body}mm"
    );
    // No column exceeds the cap (45% of body width).
    let col1_width = xs[2] - xs[1];
    assert!(col1_width <= body * 0.45 + 0.1);
}

#[test]
fn tables_emit_header_separator_and_row_rules() {
    use crate::brain::tools::doc_gen::pdf::{LayoutItem, RuleStyle, layout};

    let specs = blocks(json!([
        {"type": "table", "rows": [
            ["A", "B"],
            ["1", "2"],
            ["3", "4"]
        ]}
    ]));
    let items = layout(&specs, &StyleSpec::default());
    let heavy = items
        .iter()
        .filter(|i| {
            matches!(
                i,
                LayoutItem::Rule {
                    style: RuleStyle::HeaderSep,
                    ..
                }
            )
        })
        .count();
    let light = items
        .iter()
        .filter(|i| {
            matches!(
                i,
                LayoutItem::Rule {
                    style: RuleStyle::RowLight,
                    ..
                }
            )
        })
        .count();
    assert_eq!(heavy, 1, "exactly one header separator");
    assert_eq!(light, 1, "one light rule between the two data rows");
}

#[test]
fn styled_pdf_renders_furniture_and_page_numbers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("styled.pdf");
    let style: StyleSpec = serde_json::from_value(json!({
        "accent_color": "#0A84FF",
        "text_color": "#222222",
        "page_header": {"text": "OpenCrabs Research"},
        "page_footer": {"text": "confidential", "page_numbers": true},
        "zebra_rows": true
    }))
    .expect("style parses");
    write_pdf(
        &path,
        &blocks(json!([
            {"type": "heading", "text": "Branded Report", "level": 1},
            {"type": "table", "rows": [["A", "B"], ["1", "2"], ["3", "4"], ["5", "6"]]}
        ])),
        "Branded",
        &style,
    )
    .expect("styled pdf written");
    let text = pdf_extract::extract_text(&path).expect("pdf extracts");
    assert!(text.contains("Branded Report"));
    assert!(text.contains("OpenCrabs Research"));
    assert!(text.contains("confidential"));
    assert!(text.contains("Page 1 of 1"));
}

#[test]
fn invalid_hex_colors_fall_back_to_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("badhex.pdf");
    let style: StyleSpec = serde_json::from_value(json!({
        "accent_color": "not-a-color",
        "text_color": "#zzzzzz"
    }))
    .expect("style parses");
    write_pdf(
        &path,
        &blocks(json!([{"type": "heading", "text": "Still Works"}])),
        "Bad hex",
        &style,
    )
    .expect("bad hex never fails generation");
    let text = pdf_extract::extract_text(&path).expect("pdf extracts");
    assert!(text.contains("Still Works"));
}
