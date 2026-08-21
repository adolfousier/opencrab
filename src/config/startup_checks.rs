//! Startup config diagnostics (#477): non-fatal warnings for config
//! mistakes that otherwise produce silent, confusing behavior. Pure
//! function so the rules are unit tested; the caller logs each line once
//! at startup (never on hot reload, which would spam).

use std::sync::{LazyLock, Mutex};

use crate::config::Config;

/// Warnings from this process's startup, kept so surfaces that come up after
/// the check can still show them.
///
/// Unlike `take_typo_warnings`, reading this does NOT drain: the TUI and the
/// channels are both meant to see the same lines, and whichever initialised
/// first would otherwise consume them for everyone else.
static STARTUP_WARNINGS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Record this process's startup warnings for later display. Replaces any
/// previous set, so a caller that runs the check twice does not double them.
pub fn record_startup_warnings(warnings: &[String]) {
    let mut slot = STARTUP_WARNINGS.lock().unwrap_or_else(|e| e.into_inner());
    slot.clear();
    slot.extend_from_slice(warnings);
}

/// This process's startup warnings, for a surface that wants to show them.
pub fn recorded_startup_warnings() -> Vec<String> {
    STARTUP_WARNINGS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Clear the recorded warnings. Tests only: the store is process-global, so a
/// test that records must not leak into the next one.
pub fn clear_startup_warnings_for_test() {
    STARTUP_WARNINGS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    NOTIFIED_SCOPES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// Scopes already shown the warnings, so a chat is told once rather than on
/// every message.
static NOTIFIED_SCOPES: LazyLock<Mutex<std::collections::HashSet<String>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

/// The warning block to show in `scope`, or `None` if there is nothing to say
/// or `scope` has already been told.
///
/// `scope` identifies one destination (a chat, a channel session). A channel
/// has no startup of its own to hang this on, so it is delivered on first
/// contact instead; repeating it every message would be worse than the silence
/// it replaces.
pub fn channel_notice_for(scope: &str) -> Option<String> {
    let warnings = recorded_startup_warnings();
    if warnings.is_empty() {
        return None;
    }
    let mut seen = NOTIFIED_SCOPES.lock().unwrap_or_else(|e| e.into_inner());
    if !seen.insert(scope.to_string()) {
        return None;
    }
    Some(format!(
        "⚠️ Config warnings:\n{}",
        warnings
            .iter()
            .map(|w| format!("• {w}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

/// Collect startup warning lines for `config`. `raw_toml` is the config
/// file's raw text when available, used to spot keys the schema silently
/// ignores (serde drops unknown keys without a peep — the root of the
/// "why does my config change do nothing" archaeology this exists to end).
pub fn startup_warnings(config: &Config, raw_toml: Option<&str>) -> Vec<String> {
    let mut warns = Vec::new();

    // 1. `working_directory` is NOT a config key: the schema has no such
    //    field, so serde ignores it silently while the agent falls back to
    //    its own cwd — RSI cron, sub-agents and command_code start
    //    somewhere unexpected with zero errors. Flag the ignored key, and
    //    flag a nonexistent path on top.
    if let Some(raw) = raw_toml
        && let Ok(value) = raw.parse::<toml::Value>()
    {
        let candidates = [
            value.get("working_directory"),
            value.get("agent").and_then(|a| a.get("working_directory")),
        ];
        for c in candidates.into_iter().flatten() {
            if let Some(path) = c.as_str() {
                let expanded = expand_home(path);
                let missing = if std::path::Path::new(&expanded).is_dir() {
                    ""
                } else {
                    " — and the path does not exist on disk"
                };
                warns.push(format!(
                    "config: working_directory = \"{path}\" has NO effect — the schema has \
                     no such key (unknown keys are silently ignored). The working directory \
                     is per-session: use /cd, or set it when creating the session{missing}"
                ));
            }
        }
    }

    // 2. default_provider without force_default: new sessions get the
    //    default, but every already-open session keeps its own pair
    //    (isolation by design), so a config change looks like it did
    //    nothing. Hint at the #466 escape hatch.
    if let Some(dp) = config.agent.default_provider.as_deref() {
        match crate::brain::provider::factory::provider_config_by_name(config, dp) {
            None => warns.push(format!(
                "config: [agent] default_provider = \"{dp}\" does not match any configured \
                 provider section — new sessions will fall back to the priority list"
            )),
            Some(section) if !section.force_default => warns.push(format!(
                "config: default_provider = \"{dp}\" applies to NEW sessions only; existing \
                 sessions keep their stored pair. Add force_default = true under \
                 [providers.{dp}] to push the default to all sessions on config reload"
            )),
            Some(_) => {}
        }
    }

    // 3. Fallback-chain entries naming a provider that does not exist. The
    //    chain is resolved per attempt and an unresolvable name is skipped,
    //    which is correct behaviour and stays that way — but the skip is
    //    silent, so a typo reads as the next provider in the chain answering
    //    instead. Observed live: `modelscope-qwen37max` for the configured
    //    `modelscope-qwen37-max`, one missing hyphen, and the model the user
    //    picked was quietly answered by the entry after it. `default_provider`
    //    is already covered above; the chain was not.
    if let Some(fb) = config.providers.fallback.as_ref() {
        for (field, names) in [("providers", &fb.providers), ("vision", &fb.vision)] {
            for name in names {
                if crate::brain::provider::factory::provider_config_by_name(config, name).is_some()
                {
                    continue;
                }
                let hint = nearest_provider_name(config, name)
                    .map(|near| format!(" — did you mean \"{near}\"?"))
                    .unwrap_or_default();
                warns.push(format!(
                    "config: [providers.fallback] {field} lists \"{name}\", which matches no \
                     configured provider — it will be skipped silently{hint}"
                ));
            }
        }
    }

    warns
}

/// The configured provider whose name differs from `name` only by separators.
///
/// Deliberately narrow. The typos this catches are punctuation slips in long
/// namespaced ids (`modelscope-qwen37max` for `modelscope-qwen37-max`), where
/// an exact match on the letters and digits is strong evidence and costs no
/// judgement. A general edit distance would start guessing between providers
/// that are genuinely different.
fn nearest_provider_name<'a>(config: &'a Config, name: &str) -> Option<&'a str> {
    let squash = |s: &str| {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let target = squash(name);
    config
        .providers
        .custom
        .as_ref()?
        .keys()
        .find(|candidate| squash(candidate) == target)
        .map(String::as_str)
}

fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().to_string();
    }
    path.to_string()
}
