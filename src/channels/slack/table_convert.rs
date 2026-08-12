//! Rewrite markdown tables into Slack's own shape (#1016).
//!
//! Slack has no table markup, so a pipe table reaches the channel as its own
//! source in a proportional font where no column lines up. Block Kit has no
//! table primitive to convert into either, so the table has to stop being a
//! table.
//!
//! It becomes what a human formatting for Slack writes instead: the first
//! column as a bold label, every other column as a `└` continuation naming its
//! header. That is the layout the model produces when asked to format for
//! Slack directly, so the mechanical path and the prompted one agree.
//!
//! ```text
//! | Metric        | Before | After |        *RestartCount*
//! |---------------|--------|-------|   ->   └ Before: `272`
//! | RestartCount  | 272    | 0     |        └ After: `0`
//! ```
//!
//! A two-column table collapses to one line, `*label* — value`, because a
//! continuation for a single value is noise.
//!
//! The prompt tells the model not to emit tables at all
//! (`formatting_prompt`). This is the net under that: guidance shapes what the
//! model chooses, and a table that arrives anyway still has to render.

/// A parsed pipe table.
struct Table {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

/// Split a table line into trimmed cells, dropping the leading and trailing
/// empties that `|a|b|` produces.
fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

/// Whether `line` is a separator row (`|---|:--:|`), which is what makes the
/// line above it a header rather than ordinary prose containing pipes.
fn is_separator(line: &str) -> bool {
    let c = cells(line);
    !c.is_empty()
        && c.iter().all(|cell| {
            !cell.is_empty()
                && cell.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
                && cell.contains('-')
        })
}

/// Whether `line` could be a table row at all.
fn looks_like_row(line: &str) -> bool {
    line.trim().starts_with('|') && line.trim().len() > 1
}

/// Render one table in Slack's shape.
fn render(table: &Table) -> String {
    let mut out = Vec::new();
    for row in &table.rows {
        let label = row.first().map(|s| s.as_str()).unwrap_or("");
        if label.is_empty() && row.iter().all(|c| c.is_empty()) {
            continue;
        }
        // Two columns: one line carries it. A continuation under a single
        // value reads as ceremony.
        if table.header.len() <= 2 {
            let value = row.get(1).map(|s| s.as_str()).unwrap_or("");
            if value.is_empty() {
                out.push(format!("*{label}*"));
            } else {
                out.push(format!("*{label}* — {value}"));
            }
            continue;
        }
        out.push(format!("*{label}*"));
        for (i, cell) in row.iter().enumerate().skip(1) {
            if cell.is_empty() {
                continue;
            }
            let head = table.header.get(i).map(|s| s.as_str()).unwrap_or("");
            if head.is_empty() {
                out.push(format!("└ {cell}"));
            } else {
                out.push(format!("└ {head}: {cell}"));
            }
        }
    }
    out.join("\n")
}

/// Rewrite ATX headings as Slack section titles.
///
/// `## Container state` becomes `*CONTAINER STATE*` preceded by a `---` rule,
/// which `blocks::blocks_from_mrkdwn` turns into a real divider. Slack has no
/// heading syntax, so an unconverted `##` renders as two literal hashes.
///
/// Block Kit does have a `header` block, but it takes plain text only: no
/// inline code, no bold, no links, and a 150-character cap. A heading carrying
/// any of those would lose it, so bold caps in an ordinary section keeps more
/// of the author's intent than the block named after the job.
///
/// The rule is omitted before the first heading, where a divider would open
/// the message with a horizontal line.
fn headings_to_slack(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_fence = false;
    let mut seen_heading = false;

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            out.push(line.to_string());
            continue;
        }
        let trimmed = line.trim_start();
        let is_heading = !in_fence && trimmed.starts_with('#');
        if !is_heading {
            out.push(line.to_string());
            continue;
        }
        let title = trimmed.trim_start_matches('#').trim();
        if title.is_empty() {
            out.push(line.to_string());
            continue;
        }
        if seen_heading {
            out.push("---".to_string());
        }
        seen_heading = true;
        out.push(format!("*{}*", title.to_uppercase()));
    }

    out.join("\n")
}

/// Rewrite the structured markdown Slack cannot render: tables, then headings.
///
/// Tables first, so a heading introduced by this pass is never mistaken for
/// table content.
pub(crate) fn structure_to_slack(text: &str) -> String {
    headings_to_slack(&tables_to_slack(text))
}

/// Rewrite every markdown table in `text`, leaving everything else alone.
///
/// Tables inside fenced code blocks are untouched: a fence is someone showing
/// the syntax, not asking for it to render.
pub(crate) fn tables_to_slack(text: &str) -> String {
    if !text.contains('|') {
        return text.to_string();
    }

    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    let mut in_fence = false;

    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            out.push(line.to_string());
            i += 1;
            continue;
        }
        if in_fence {
            out.push(line.to_string());
            i += 1;
            continue;
        }

        // A header row is only a header when a separator follows it.
        let has_table = looks_like_row(line)
            && i + 1 < lines.len()
            && looks_like_row(lines[i + 1])
            && is_separator(lines[i + 1]);

        if !has_table {
            out.push(line.to_string());
            i += 1;
            continue;
        }

        let header = cells(line);
        let mut rows = Vec::new();
        let mut j = i + 2;
        while j < lines.len() && looks_like_row(lines[j]) && !is_separator(lines[j]) {
            rows.push(cells(lines[j]));
            j += 1;
        }

        let rendered = render(&Table { header, rows });
        if !rendered.is_empty() {
            out.push(rendered);
        }
        i = j;
    }

    out.join("\n")
}
