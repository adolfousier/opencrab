//! Paging and filtering for the model picker.
//!
//! The picker sent the whole list twice: enumerated in the message text, and
//! again as one button per model. With OpenRouter's several hundred the text
//! alone passed Telegram's 4096 characters, `editMessageText` answered
//! MESSAGE_TOO_LONG, and the only trace was a log line. From the chat the
//! picker did nothing at all.
//!
//! A cap alone would have hidden most of the catalogue, so this pages instead,
//! and takes a filter so a name can be narrowed to rather than scrolled to.

/// Models per page. Small enough that the page plus its navigation row stays
/// well inside the message limit even with long vendor-prefixed names.
pub(crate) const MODEL_PAGE_SIZE: usize = 20;

/// One page of a provider's catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelPage {
    /// Models on this page, in catalogue order.
    pub(crate) models: Vec<String>,
    /// Zero-based, already clamped to the last page that exists.
    pub(crate) page: usize,
    /// Always at least 1, so "Page 1/1" reads correctly for an empty list.
    pub(crate) total_pages: usize,
    /// How many models matched the filter, across all pages.
    pub(crate) matched: usize,
    /// The filter in force, if any.
    pub(crate) filter: Option<String>,
}

impl ModelPage {
    pub(crate) fn has_prev(&self) -> bool {
        self.page > 0
    }
    pub(crate) fn has_next(&self) -> bool {
        self.page + 1 < self.total_pages
    }
}

/// Select the models to show, applying `filter` then paging.
///
/// The filter is a case-insensitive substring over the full model id, so
/// `gpt`, `claude` or a vendor prefix all narrow usefully. An out-of-range
/// page clamps to the last one rather than showing nothing, which is what a
/// stale button from an earlier filter would otherwise do.
pub(crate) fn page_of(models: &[String], page: usize, filter: Option<&str>) -> ModelPage {
    let needle = filter
        .map(|f| f.trim().to_lowercase())
        .filter(|f| !f.is_empty());
    let matched: Vec<String> = match &needle {
        Some(f) => models
            .iter()
            .filter(|m| m.to_lowercase().contains(f.as_str()))
            .cloned()
            .collect(),
        None => models.to_vec(),
    };

    let total_pages = matched.len().div_ceil(MODEL_PAGE_SIZE).max(1);
    let page = page.min(total_pages - 1);
    let start = page * MODEL_PAGE_SIZE;

    ModelPage {
        models: matched
            .iter()
            .skip(start)
            .take(MODEL_PAGE_SIZE)
            .cloned()
            .collect(),
        page,
        total_pages,
        matched: matched.len(),
        filter: needle,
    }
}

/// The picker's message text: a header, the filter/page status and a hint —
/// never an enumeration of the page's models. The buttons carry the models,
/// so repeating them in text is what overflowed the limit in the first place;
/// listing just the current page was still pure duplication (#1149).
pub(crate) fn page_text(
    display_name: &str,
    current_model: &str,
    total_models: usize,
    page: &ModelPage,
) -> String {
    let mut lines = vec![format!("🤖 *{display_name} Models*")];
    lines.push(format!("Current: `{current_model}`"));

    match (&page.filter, page.matched) {
        (Some(f), 0) => {
            lines.push(String::new());
            lines.push(format!("No model matches `{f}` (of {total_models})."));
            lines.push("Send /models with a different filter to search again.".into());
            return lines.join("\n");
        }
        (Some(f), n) => lines.push(format!("Filter: `{f}` — {n} of {total_models} match")),
        (None, n) => lines.push(format!("{n} available")),
    }

    if page.total_pages > 1 {
        lines.push(format!(
            "Page {}/{} — use ◀ ▶ to page",
            page.page + 1,
            page.total_pages
        ));
    }
    lines.push(String::new());
    lines.push("Tap a model below (✓ = current).".into());
    if page.total_pages > 1 {
        lines.push("/models <text> filters by name.".into());
    }
    lines.join("\n")
}

/// Build the picker's keyboard for one page: a button per model, then a
/// navigation row when there is more than one page.
///
/// The page's own models are recorded as the rendered list, so a long model
/// name encoded as its position resolves against exactly what was on screen
/// (see `model_menu`). Using catalogue positions here would resolve a tap on
/// page 3 to a model from page 1.
pub(crate) fn page_keyboard(
    provider_name: &str,
    current_model: &str,
    page: &ModelPage,
) -> Vec<Vec<teloxide::types::InlineKeyboardButton>> {
    use teloxide::types::InlineKeyboardButton;

    crate::channels::model_menu::remember(provider_name, &page.models);

    let mut rows: Vec<Vec<InlineKeyboardButton>> = page
        .models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let display = if m == current_model {
                format!("✓ {m}")
            } else {
                m.clone()
            };
            let data = crate::channels::commands::model_button_callback_data(provider_name, m, i);
            vec![InlineKeyboardButton::callback(display, data)]
        })
        .collect();

    if page.total_pages > 1 {
        let mut nav = Vec::new();
        if page.has_prev() {
            nav.push(InlineKeyboardButton::callback(
                "◀ Prev".to_string(),
                nav_callback_data(provider_name, page.page - 1, page.filter.as_deref()),
            ));
        }
        nav.push(InlineKeyboardButton::callback(
            format!("{}/{}", page.page + 1, page.total_pages),
            "noop".to_string(),
        ));
        if page.has_next() {
            nav.push(InlineKeyboardButton::callback(
                "Next ▶".to_string(),
                nav_callback_data(provider_name, page.page + 1, page.filter.as_deref()),
            ));
        }
        rows.push(nav);
    }
    rows
}

/// Callback payload for a page button: `mp:<page>|<provider>[|<filter>]`.
///
/// Pipe-separated for the same reason the model buttons are: a provider is
/// `custom:<name>` and a model can carry `:free`, so splitting on `:` folds
/// them into each other. Telegram caps this at 64 bytes, so the filter is
/// dropped rather than truncated when it would not fit: a page that loses its
/// filter shows more models than intended, while a truncated one would show
/// the wrong ones.
pub(crate) fn nav_callback_data(provider_name: &str, page: usize, filter: Option<&str>) -> String {
    let base = format!("mp:{page}|{provider_name}");
    match filter {
        Some(f) if base.len() + f.len() < 64 => format!("{base}|{f}"),
        _ => base,
    }
}

/// Parse a page-button payload back into (page, provider, filter).
pub(crate) fn parse_nav_callback(data: &str) -> Option<(usize, String, Option<String>)> {
    let rest = data.strip_prefix("mp:")?;
    let (page_str, tail) = rest.split_once('|')?;
    let page = page_str.parse().ok()?;
    match tail.split_once('|') {
        Some((provider, filter)) => Some((page, provider.to_string(), Some(filter.to_string()))),
        None => Some((page, tail.to_string(), None)),
    }
}
