//! Schema-independent AST for Telegram rich messages.
//!
//! Markdown text is parsed into this structure (see [`super::parse`]). The AST
//! is deliberately independent of any wire format so that:
//!
//! * the parser and its tests don't churn when Telegram's `InputRichMessage`
//!   field schema is finalized, and
//! * the same AST drives both the rich-first path (serialized to RichBlocks)
//!   and the HTML fallback ([`super::render_html`]).

/// A block-level element.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// ATX heading, `level` in `1..=6`.
    Heading { level: u8, content: Vec<Inline> },
    /// A run of inline content terminated by a blank line.
    Paragraph(Vec<Inline>),
    /// A bulleted or numbered list (items may carry task checkboxes and nested
    /// child blocks).
    List(List),
    /// A GitHub-flavored pipe table.
    Table(Table),
    /// A fenced code block with an optional language tag.
    Code { lang: Option<String>, text: String },
    /// A block quotation; may contain nested blocks.
    Quote(Vec<Block>),
    /// Display (block-level) math, e.g. `$$ ... $$`.
    Math(String),
    /// A horizontal rule / divider.
    Divider,
}

/// A bulleted or numbered list.
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    /// `true` for `1.`/`2.` ordered lists, `false` for `-`/`*`/`+` bullets.
    pub ordered: bool,
    pub items: Vec<ListItem>,
}

/// A single list item.
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    /// `None` for a normal item; `Some(checked)` for a `- [ ]` / `- [x]` task.
    pub task: Option<bool>,
    /// The item's own inline content (the text on the marker line).
    pub content: Vec<Inline>,
    /// Blocks indented under this item (typically a nested [`List`]).
    pub children: Vec<Block>,
}

/// Column alignment for a [`Table`], derived from the separator row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// A GitHub-flavored pipe table.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Per-column alignment; same length as `header`.
    pub align: Vec<Align>,
    /// Header cells, each a run of inline content.
    pub header: Vec<Vec<Inline>>,
    /// Body rows; each row is a list of cells.
    pub rows: Vec<Vec<Vec<Inline>>>,
}

/// An inline-level element.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Bold(Vec<Inline>),
    Italic(Vec<Inline>),
    Strike(Vec<Inline>),
    /// Inline `code` — content is literal, never re-parsed.
    Code(String),
    /// Inline `$math$` — content is literal, never re-parsed.
    Math(String),
    Link {
        content: Vec<Inline>,
        url: String,
    },
}
