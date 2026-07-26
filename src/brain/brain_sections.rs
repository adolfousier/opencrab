//! Pull just the relevant sections out of a brain file (#800).
//!
//! `load_brain_file` was all-or-nothing, so consulting MEMORY.md meant loading
//! all of it. Since the file is append-only and only grows, the cost of reading
//! rose exactly as it accumulated more worth reading, and most of the payload
//! was irrelevant to any given turn. Not calling it was locally rational, which
//! explains the file being written constantly and read almost never.
//!
//! Matching returns COMPLETE sections, never character snippets. A brain file
//! is a set of rules, and half a rule is worse than none: it reads as complete
//! while dropping the qualifier that made it correct. Section granularity keeps
//! every returned rule self-contained, and keeps the result explainable rather
//! than rank-dependent.
//!
//! Runs on text already in hand, so unlike the FTS hint path it works with a
//! cold or missing index.

/// Max sections returned, so a broad query cannot dump the file back.
const MAX_SECTIONS: usize = 6;
/// Total character budget across all returned sections.
const MAX_CHARS: usize = 6000;

/// One markdown section: its heading line and everything until the next
/// heading at the same or a shallower depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub heading: String,
    pub body: String,
}

impl Section {
    /// The section as it appeared in the file.
    pub fn render(&self) -> String {
        if self.heading.is_empty() {
            self.body.clone()
        } else if self.body.trim().is_empty() {
            self.heading.clone()
        } else {
            format!("{}\n{}", self.heading, self.body)
        }
    }

    /// The section's distinct words, lowercased, for matching.
    ///
    /// Words rather than raw substrings: `contains("the")` is true of "them",
    /// so substring matching let a short common term score against a section
    /// that never mentions it, and two such accidents were enough to clear the
    /// relevance bar.
    fn words(&self) -> std::collections::HashSet<String> {
        format!("{} {}", self.heading, self.body)
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// Split markdown into sections on ATX headings (`#`, `##`, ...).
///
/// Content before the first heading becomes a leading section with an empty
/// heading, so a file with no headings at all still yields one section rather
/// than vanishing.
pub fn split_sections(content: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut heading = String::new();
    let mut body = String::new();
    let mut in_fence = false;

    for line in content.lines() {
        // A `#` inside a fenced block is code or a shell comment, not a
        // heading; treating it as one would split a rule down the middle.
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        let is_heading = !in_fence && line.trim_start().starts_with('#');

        if is_heading {
            if !heading.is_empty() || !body.trim().is_empty() {
                sections.push(Section {
                    heading: std::mem::take(&mut heading),
                    body: std::mem::take(&mut body),
                });
            }
            heading = line.to_string();
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if !heading.is_empty() || !body.trim().is_empty() {
        sections.push(Section { heading, body });
    }
    sections
}

/// What a query found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matches {
    /// The matching sections, best first, already capped.
    pub sections: Vec<Section>,
    /// How many matched but were dropped by the caps.
    pub omitted: usize,
}

impl Matches {
    /// Render for the tool result, stating omissions rather than hiding them.
    ///
    /// A capped result that reads as complete is how a partial read becomes a
    /// wrong answer, and an empty result has to say it found nothing rather
    /// than let silence be read as "no such rule exists".
    pub fn render(&self, file: &str, query: &str) -> String {
        if self.sections.is_empty() {
            return format!(
                "No section of {file} matched \"{query}\". The file may use different wording; \
                 load it without a query to read it in full."
            );
        }
        let mut out = format!(
            "{} section(s) of {file} matched \"{query}\":\n\n",
            self.sections.len()
        );
        out.push_str(
            &self
                .sections
                .iter()
                .map(Section::render)
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
        if self.omitted > 0 {
            out.push_str(&format!(
                "\n\n[{} further matching section(s) omitted for length. Narrow the query, or \
                 load {file} without a query to read it in full.]",
                self.omitted
            ));
        }
        out
    }
}

/// Find the sections of `content` relevant to `query`.
///
/// Scored by how many distinct query terms a section contains, so a section
/// covering the whole question outranks one that repeats a single common word.
/// Ties keep file order, which keeps results stable between identical calls.
pub fn find_sections(content: &str, query: &str) -> Matches {
    find_sections_with(content, query, MAX_SECTIONS, MAX_CHARS, 1)
}

/// [`find_sections`] with explicit bounds.
///
/// Automatic recall needs far tighter limits than a read the model chose to
/// make: it is paid on every turn, and it competes with the actual task for
/// attention. `min_hits` raises the bar for what counts as a match at all,
/// which is what keeps an incidental common word from injecting a section
/// nobody asked about.
pub fn find_sections_with(
    content: &str,
    query: &str,
    max_sections: usize,
    max_chars: usize,
    min_hits: usize,
) -> Matches {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| {
            t.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|t| t.len() > 2)
        .collect();

    if terms.is_empty() {
        return Matches {
            sections: Vec::new(),
            omitted: 0,
        };
    }

    let mut scored: Vec<(usize, usize, Section)> = split_sections(content)
        .into_iter()
        .enumerate()
        .filter_map(|(idx, section)| {
            let hay = section.words();
            let hits = terms.iter().filter(|t| hay.contains(t.as_str())).count();
            (hits >= min_hits.max(1)).then_some((hits, idx, section))
        })
        .collect();

    // Best score first; original order breaks ties.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let total = scored.len();
    let mut sections = Vec::new();
    let mut chars = 0usize;
    for (_, _, section) in scored {
        if sections.len() >= max_sections {
            break;
        }
        let len = section.render().chars().count();
        if chars + len > max_chars && !sections.is_empty() {
            break;
        }
        chars += len;
        sections.push(section);
    }

    let omitted = total - sections.len();
    Matches { sections, omitted }
}
