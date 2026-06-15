//! Render the AST to Telegram HTML — the fallback path used when the rich
//! `sendRichMessage` API is unavailable or rejects a message.
//!
//! Telegram HTML has no table/heading/list tags, so structure is approximated:
//! headings become bold, lists become bullet/number/checkbox lines, and tables
//! become an aligned monospace grid inside `<pre>`. Inline styling is preserved
//! everywhere except inside table cells (preformatted text can't carry tags).

use super::ast::{Align, Block, Inline, List, Table};

/// Render a block list to a Telegram-HTML string.
pub(super) fn render_html(blocks: &[Block]) -> String {
    let mut out = String::new();
    for b in blocks {
        render_block(b, 0, &mut out);
    }
    out.trim_end().to_string()
}

fn render_block(block: &Block, depth: usize, out: &mut String) {
    match block {
        Block::Heading { level, content } => {
            // No heading tags in Telegram HTML: bold, and italicize deeper
            // headings (level >= 3) so the hierarchy stays visible.
            let deep = *level >= 3;
            out.push_str("<b>");
            if deep {
                out.push_str("<i>");
            }
            out.push_str(&render_inlines(content));
            if deep {
                out.push_str("</i>");
            }
            out.push_str("</b>\n");
        }
        Block::Paragraph(content) => {
            out.push_str(&render_inlines(content));
            out.push('\n');
        }
        Block::List(list) => render_list(list, depth, out),
        Block::Table(table) => {
            out.push_str(&render_table(table));
            out.push('\n');
        }
        Block::Code { lang, text } => {
            match lang {
                Some(l) => out.push_str(&format!("<pre><code class=\"language-{}\">", escape(l))),
                None => out.push_str("<pre><code>"),
            }
            out.push_str(&escape(text));
            out.push_str("</code></pre>\n");
        }
        Block::Quote(inner) => {
            let mut buf = String::new();
            for ib in inner {
                render_block(ib, 0, &mut buf);
            }
            out.push_str("<blockquote>");
            out.push_str(buf.trim_end());
            out.push_str("</blockquote>\n");
        }
        Block::Math(expr) => {
            out.push_str("<pre>");
            out.push_str(&escape(expr));
            out.push_str("</pre>\n");
        }
        Block::Divider => out.push_str("──────────\n"),
    }
}

fn render_list(list: &List, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    for (idx, item) in list.items.iter().enumerate() {
        let bullet = match item.task {
            Some(true) => "☑".to_string(),
            Some(false) => "☐".to_string(),
            None if list.ordered => format!("{}.", idx + 1),
            None => "•".to_string(),
        };
        out.push_str(&pad);
        out.push_str(&bullet);
        out.push(' ');
        out.push_str(&render_inlines(&item.content));
        out.push('\n');
        for child in &item.children {
            render_block(child, depth + 1, out);
        }
    }
}

fn render_table(table: &Table) -> String {
    let cols = table.header.len();
    let header: Vec<String> = table.header.iter().map(|c| plain(c)).collect();
    let rows: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|r| r.iter().map(|c| plain(c)).collect())
        .collect();

    // Column widths from the widest plain-text cell in each column.
    let mut width = vec![0usize; cols];
    for (i, h) in header.iter().enumerate() {
        width[i] = width[i].max(h.chars().count());
    }
    for row in &rows {
        for (i, c) in row.iter().enumerate().take(cols) {
            width[i] = width[i].max(c.chars().count());
        }
    }

    let fmt = |cells: &[String]| -> String {
        width
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let cell = cells.get(i).map(String::as_str).unwrap_or("");
                let align = table.align.get(i).copied().unwrap_or(Align::None);
                let padded = pad_cell(cell, *w, align);
                if i + 1 < cols {
                    format!("{} | ", padded)
                } else {
                    padded
                }
            })
            .collect()
    };

    let sep: String = width
        .iter()
        .map(|w| "-".repeat(*w))
        .collect::<Vec<_>>()
        .join("-+-");

    let mut lines = vec![fmt(&header), sep];
    for row in &rows {
        lines.push(fmt(row));
    }
    format!("<pre>{}</pre>", escape(&lines.join("\n")))
}

fn pad_cell(s: &str, width: usize, align: Align) -> String {
    let len = s.chars().count();
    if len >= width {
        return s.to_string();
    }
    let total = width - len;
    match align {
        Align::Right => format!("{}{}", " ".repeat(total), s),
        Align::Center => {
            let left = total / 2;
            format!("{}{}{}", " ".repeat(left), s, " ".repeat(total - left))
        }
        Align::Left | Align::None => format!("{}{}", s, " ".repeat(total)),
    }
}

fn render_inlines(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        render_inline(i, &mut s);
    }
    s
}

fn render_inline(inline: &Inline, s: &mut String) {
    match inline {
        Inline::Text(t) => s.push_str(&escape(t)),
        Inline::Bold(c) => wrap(s, "<b>", c, "</b>"),
        Inline::Italic(c) => wrap(s, "<i>", c, "</i>"),
        Inline::Strike(c) => wrap(s, "<s>", c, "</s>"),
        Inline::Code(t) | Inline::Math(t) => {
            s.push_str("<code>");
            s.push_str(&escape(t));
            s.push_str("</code>");
        }
        Inline::Link { content, url } => {
            s.push_str(&format!("<a href=\"{}\">", escape(url)));
            for c in content {
                render_inline(c, s);
            }
            s.push_str("</a>");
        }
    }
}

fn wrap(s: &mut String, open: &str, content: &[Inline], close: &str) {
    s.push_str(open);
    for c in content {
        render_inline(c, s);
    }
    s.push_str(close);
}

/// Flatten inline content to plain text (used for table cells, which can't
/// carry styling tags inside `<pre>`).
fn plain(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        plain_one(i, &mut s);
    }
    s
}

fn plain_one(inline: &Inline, s: &mut String) {
    match inline {
        Inline::Text(t) | Inline::Code(t) | Inline::Math(t) => s.push_str(t),
        Inline::Bold(c) | Inline::Italic(c) | Inline::Strike(c) => {
            for x in c {
                plain_one(x, s);
            }
        }
        Inline::Link { content, .. } => {
            for x in content {
                plain_one(x, s);
            }
        }
    }
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
