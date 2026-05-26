//! Pure Telegram session title + label-drift helpers (testable without teloxide).
//!
//! Issue #121: naive full-title comparison reverted auto-titled DM sessions back
//! to the default `Telegram: DM …` template on every subsequent message.

/// Build the canonical session title for a Telegram chat.
pub fn build_session_title(
    is_dm: bool,
    user_name: &str,
    user_id: i64,
    chat_title: &str,
    chat_id: i64,
) -> String {
    let chat_id_suffix = format!("[chat:{chat_id}]");
    if is_dm {
        format!("Telegram: DM {user_name} ({user_id}) {chat_id_suffix}")
    } else {
        format!("Telegram: {chat_title} {chat_id_suffix}")
    }
}

/// Legacy title format (pre suffix) for migration lookups.
pub fn build_legacy_session_title(is_dm: bool, user_name: &str, user_id: i64, chat_title: &str) -> String {
    if is_dm {
        format!("Telegram: DM {user_name} ({user_id})")
    } else {
        format!("Telegram: {chat_title}")
    }
}

pub fn chat_id_suffix(chat_id: i64) -> String {
    format!("[chat:{chat_id}]")
}

/// True when a session exceeded the configured idle window (same rule as handler suffix path).
pub fn session_idle_expired(updated_at: chrono::DateTime<chrono::Utc>, idle_hours: Option<f64>) -> bool {
    idle_hours.is_some_and(|h| {
        let elapsed = (chrono::Utc::now() - updated_at).num_seconds();
        elapsed > (h * 3600.0) as i64
    })
}

/// Handler resolve policy: explicit chat binding wins over suffix `updated_at` winner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveSource {
    ChatBound,
    Suffix,
    Create,
}

pub fn choose_resolve_source(
    chat_bound: Option<uuid::Uuid>,
    bound_archived: bool,
    suffix_match: Option<uuid::Uuid>,
) -> ResolveSource {
    if chat_bound.is_some() && !bound_archived {
        ResolveSource::ChatBound
    } else if suffix_match.is_some() {
        ResolveSource::Suffix
    } else {
        ResolveSource::Create
    }
}

/// Whether to overwrite a stored session title with the freshly built template.
///
/// - Default DM titles: refresh when the template default changed (display name).
/// - Auto-titled / custom DM titles: never clobber (issue #121).
/// - Telegram groups: refresh when the visible group label changed (suffix stable).
pub fn should_refresh_label(stored: &str, template: &str) -> bool {
    if stored == template {
        return false;
    }

    if crate::brain::agent::service::AgentService::is_default_channel_title(stored) {
        return crate::brain::agent::service::AgentService::is_default_channel_title(template)
            && stored != template;
    }

    if is_telegram_group_session_title(stored) && is_telegram_group_session_title(template) {
        return telegram_middle_label(stored) != telegram_middle_label(template);
    }

    false
}

fn is_telegram_group_session_title(title: &str) -> bool {
    let Some(rest) = title.strip_prefix("Telegram: ") else {
        return false;
    };
    !rest.starts_with("DM ") && title.contains("[chat:")
}

fn telegram_middle_label(title: &str) -> String {
    let body = title
        .strip_prefix("Telegram: ")
        .unwrap_or(title)
        .trim();
    let suffix = crate::brain::agent::service::AgentService::extract_chat_id_suffix(title);
    if suffix.is_empty() {
        return body.to_string();
    }
    body.strip_suffix(suffix)
        .unwrap_or(body)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dm_template_format() {
        let t = build_session_title(true, "Alice", 123, "", 456);
        assert_eq!(t, "Telegram: DM Alice (123) [chat:456]");
    }

    #[test]
    fn should_not_clobber_auto_titled_dm() {
        let auto = "Telegram: Fix deploy [chat:133526395]";
        let template = build_session_title(true, "Alexey", 133526395, "", 133526395);
        assert!(!should_refresh_label(auto, template));
    }

    #[test]
    fn should_refresh_group_rename() {
        let old = "Telegram: Old Group [chat:-1]";
        let new = "Telegram: New Group [chat:-1]";
        assert!(should_refresh_label(old, new));
    }

    #[test]
    fn default_dm_still_refreshes_on_name_change() {
        let old = build_session_title(true, "Alice", 1, "", 99);
        let new = build_session_title(true, "Bob", 1, "", 99);
        assert!(should_refresh_label(&old, &new));
    }

    #[test]
    fn chat_bound_wins_over_suffix_candidate() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        assert_eq!(
            choose_resolve_source(Some(a), false, Some(b)),
            ResolveSource::ChatBound
        );
    }

    #[test]
    fn archived_bound_falls_through_to_suffix() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        assert_eq!(
            choose_resolve_source(Some(a), true, Some(b)),
            ResolveSource::Suffix
        );
    }

    #[test]
    fn session_idle_expired_within_and_past_window() {
        let recent = chrono::Utc::now() - chrono::Duration::minutes(30);
        assert!(!session_idle_expired(recent, Some(1.0)));

        let stale = chrono::Utc::now() - chrono::Duration::hours(2);
        assert!(session_idle_expired(stale, Some(1.0)));
        assert!(!session_idle_expired(stale, None));
    }

    #[test]
    fn session_idle_expired_boundary_not_yet_expired() {
        let at_limit = chrono::Utc::now() - chrono::Duration::seconds(3600);
        assert!(!session_idle_expired(at_limit, Some(1.0)));
        let past_limit = chrono::Utc::now() - chrono::Duration::seconds(3601);
        assert!(session_idle_expired(past_limit, Some(1.0)));
    }
}
