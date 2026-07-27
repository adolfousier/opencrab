//! Activity feed data service — parses `~/.opencrabs/rsi/improvements.md`
//! into a uniform `Vec<McActivity>` for the activity panel.
//!
//! `improvements.md` is the RSI loop's append-only journal. Each entry
//! is a markdown block of the shape:
//!
//! ```md
//! ## [Applied] Add conciseness guideline
//!
//! **Date:** 2026-04-12 23:01 UTC
//! **Target:** SOUL.md
//! **Rationale:** Users consistently prefer shorter responses
//! **Status:** Applied
//! ```
//!
//! The parser is forgiving: missing fields fall back to sensible
//! defaults rather than dropping the entry.

use super::types::{McActivity, McActivityLevel};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

/// Read up to `limit` newest entries from the improvements log.
pub fn recent(limit: usize) -> Vec<McActivity> {
    let path = crate::config::opencrabs_home()
        .join("rsi")
        .join("improvements.md");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    parse_improvements_md(&content, limit)
}

/// Pure parser — exposed for tests.
pub fn parse_improvements_md(content: &str, limit: usize) -> Vec<McActivity> {
    let mut entries: Vec<McActivity> = Vec::new();
    let mut current: Option<EntryDraft> = None;

    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            // Flush the previous entry, start a new one.
            if let Some(draft) = current.take() {
                entries.push(draft.finish());
            }
            current = Some(EntryDraft::from_header(rest));
        } else if starts_a_headerless_entry(line, current.as_ref()) {
            // A second `**Date:**` inside one draft means the journal wrote a
            // block without a `## ` header. Its fields belong to a new entry,
            // not to the one above it: letting them through overwrote the
            // previous entry's date, target, rationale and status, which lost
            // one entry outright and misdated its neighbour (#841).
            if let Some(draft) = current.take() {
                entries.push(draft.finish());
            }
            let mut draft = EntryDraft::from_header("");
            apply_field_line(line, &mut draft);
            current = Some(draft);
        } else if let Some(draft) = current.as_mut() {
            apply_field_line(line, draft);
        }
    }
    if let Some(draft) = current.take() {
        entries.push(draft.finish());
    }

    // The journal is append-only with oldest at the top — surface
    // newest first so the panel matches the rest of OpenCrabs.
    entries.reverse();
    entries.truncate(limit);
    entries
}

/// Does this line open an entry the journal wrote without a `## ` header?
///
/// The only signal available is a `**Date:**` arriving when the open draft
/// already saw one. Blocks like this exist in the journal, separated from the
/// entry above by a bare `---`, which the header check does not recognise.
fn starts_a_headerless_entry(line: &str, current: Option<&EntryDraft>) -> bool {
    line.trim_start().starts_with("**Date:**") && current.is_some_and(|d| d.saw_date)
}

struct EntryDraft {
    title: String,
    status_from_header: String,
    date: Option<DateTime<Utc>>,
    /// Whether a `**Date:**` line was seen at all, parseable or not. Distinct
    /// from `date.is_some()` so an unparseable date still marks the boundary
    /// of the next headerless block.
    saw_date: bool,
    target: Option<String>,
    rationale: Option<String>,
    status_field: Option<String>,
}

impl EntryDraft {
    /// `header` is everything after `"## "` — typically `"[Applied] Title"`.
    ///
    /// The config-sync writer instead leads with the timestamp and carries no
    /// `**Date:**` field at all, so the header is also checked for one.
    fn from_header(header: &str) -> Self {
        let (date, header) = match split_leading_date(header) {
            Some((date, rest)) => (Some(date), rest),
            None => (None, header.trim().to_string()),
        };
        let (status_from_header, title) = match header.strip_prefix('[') {
            Some(rest) => match rest.split_once(']') {
                Some((status, title)) => (status.trim().to_string(), title.trim().to_string()),
                None => (String::new(), header.trim().to_string()),
            },
            None => (String::new(), header.trim().to_string()),
        };
        Self {
            title,
            status_from_header,
            date,
            saw_date: false,
            target: None,
            rationale: None,
            status_field: None,
        }
    }

    fn finish(self) -> McActivity {
        let level = level_from_status(self.status_field.as_deref(), &self.status_from_header);
        let detail = build_detail(
            &self.title,
            self.target.as_deref(),
            self.rationale.as_deref(),
        );
        McActivity {
            // No `Utc::now()` fallback: an entry whose date did not parse is
            // undated, and saying so is honest where "10s ago" was not (#841).
            timestamp: self.date,
            detail,
            level,
            source: "rsi".to_string(),
        }
    }
}

fn apply_field_line(line: &str, draft: &mut EntryDraft) {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("**Date:**") {
        draft.saw_date = true;
        // Only on success. Assigning the `None` from an unparseable date is
        // what erased a neighbour's correct timestamp (#841).
        if let Some(date) = parse_date(rest.trim()) {
            draft.date = Some(date);
        }
    } else if let Some(rest) = trimmed.strip_prefix("**Target:**") {
        let target = rest.trim();
        if !target.is_empty() && target != "(none)" {
            draft.target = Some(target.to_string());
        }
    } else if let Some(rest) = trimmed.strip_prefix("**Rationale:**") {
        let rationale = rest.trim();
        if !rationale.is_empty() && rationale != "(none)" {
            draft.rationale = Some(rationale.to_string());
        }
    } else if let Some(rest) = trimmed.strip_prefix("**Status:**") {
        let s = rest.trim();
        if !s.is_empty() {
            draft.status_field = Some(s.to_string());
        }
    }
}

/// Split a header that leads with a timestamp into that date and the rest.
///
/// Matches `2026-07-26 23:33 UTC - config.toml config sync`, whatever the
/// separator: the date is the first three whitespace-separated tokens, and
/// everything after it is the title with any leading punctuation removed.
fn split_leading_date(header: &str) -> Option<(DateTime<Utc>, String)> {
    let header = header.trim();
    let mut parts = header.splitn(4, ' ');
    let day = parts.next()?;
    let time = parts.next()?;
    let zone = parts.next()?;
    if zone != "UTC" {
        return None;
    }
    let date = parse_date(&format!("{day} {time}"))?;
    let title = parts
        .next()
        .unwrap_or("")
        .trim_start_matches(['-', '\u{2013}', '\u{2014}', ':', ' '])
        .trim()
        .to_string();
    Some((date, title))
}

/// Parse the journal's `YYYY-MM-DD HH:MM UTC` format.
fn parse_date(s: &str) -> Option<DateTime<Utc>> {
    // Strip trailing " UTC" if present so the format string doesn't
    // need a hard-coded timezone token.
    let core = s.strip_suffix(" UTC").unwrap_or(s);
    NaiveDateTime::parse_from_str(core, "%Y-%m-%d %H:%M")
        .ok()
        .and_then(|naive| Utc.from_local_datetime(&naive).single())
}

fn level_from_status(status_field: Option<&str>, header_status: &str) -> McActivityLevel {
    let status = status_field.unwrap_or(header_status).to_lowercase();
    match status.as_str() {
        "applied" => McActivityLevel::Success,
        "failed" | "error" => McActivityLevel::Error,
        "warn" | "warning" | "reverted" | "rolled-back" => McActivityLevel::Warn,
        _ => McActivityLevel::Info,
    }
}

fn build_detail(title: &str, target: Option<&str>, rationale: Option<&str>) -> String {
    match (target, rationale) {
        (Some(t), Some(r)) => format!("{title} → {t} ({r})"),
        (Some(t), None) => format!("{title} → {t}"),
        (None, Some(r)) => format!("{title} ({r})"),
        (None, None) => title.to_string(),
    }
}
