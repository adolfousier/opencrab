//! `provider/model` pair parsing, shared by every non-interactive switch
//! surface (#461 family: CLI set-model, direct-arg /models on channels and
//! the TUI). The FIRST slash splits, so model names that contain slashes
//! survive intact (`openrouter/tencent/hy3:free` → `openrouter` +
//! `tencent/hy3:free`).

pub fn parse_pair(pair: &str) -> Result<(String, String), String> {
    match pair.split_once('/') {
        Some((provider, model)) if !provider.trim().is_empty() && !model.trim().is_empty() => {
            Ok((provider.trim().to_string(), model.trim().to_string()))
        }
        _ => Err(format!(
            "'{pair}' is not a provider/model pair — expected e.g. \
             openrouter/tencent/hy3:free (the first slash splits)"
        )),
    }
}
