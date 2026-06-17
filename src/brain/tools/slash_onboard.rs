//! Channel-capable `/onboard:*` handlers.
//!
//! The TUI onboarding wizard is an interactive screen, so on channels
//! (Telegram/Discord/Slack/WhatsApp) the `/onboard:*` steps were unreachable.
//! These text-driven handlers expose the same setup over a chat: called with
//! no args they print a menu; called with args they write the exact same
//! config/keys the wizard writes. Routed from `slash_command` via the
//! `/onboard:<step>` prefix.
//!
//! `provider` is intentionally absent — `/models` already covers it on
//! channels. `brain` and the brain-directory change stay TUI-only.

use super::error::Result;
use super::r#trait::ToolResult;
use crate::config::Config;

/// Route `/onboard:<sub>` to the matching handler.
pub(crate) fn dispatch(sub: &str, args: &str) -> Result<ToolResult> {
    match sub.trim().to_lowercase().as_str() {
        "image" => onboard_image(args),
        "voice" => onboard_voice(args),
        "channels" => onboard_channels(args),
        "brain" => Ok(ToolResult::success(
            "Brain/persona setup edits multiple markdown files and is TUI-only \
             (type /onboard:brain in the desktop app). To tweak persona text from \
             here, read/edit the brain files via the brain tools."
                .into(),
        )),
        other => Ok(ToolResult::error(format!(
            "Unknown onboarding step '{other}'. Available on channels: image, voice, channels."
        ))),
    }
}

/// Map the active provider id (`anthropic`, `custom:foo`, …) to its config
/// section (`providers.anthropic`, `providers.custom.foo`). `None` when no
/// provider is configured.
fn active_provider_section(config: &Config) -> Option<String> {
    let (id, _model) = config.providers.active_provider_and_model();
    match id.as_str() {
        "none" => None,
        s => match s.strip_prefix("custom:") {
            Some(name) => Some(format!("providers.custom.{name}")),
            None => Some(format!("providers.{s}")),
        },
    }
}

/// Split `"first rest of the string"` into (`"first"`, `"rest of the string"`).
fn split_first(s: &str) -> (&str, &str) {
    let s = s.trim();
    match s.split_once(char::is_whitespace) {
        Some((a, b)) => (a, b.trim()),
        None => (s, ""),
    }
}

const IMAGE_MENU: &str = "Image setup — pick one:\n\
    • `/onboard:image gemini <GOOGLE_AI_KEY>` — Google vision + image generation (Nano Banana).\n\
    • `/onboard:image provider <VISION_MODEL>` — use your ACTIVE provider's vision model \
    (OpenAI-compatible, no extra key). e.g. `/onboard:image provider mimo-v2.5-pro`.\n\
    Ask the user which they want and re-run with the argument.";

/// `/onboard:image [gemini <key> | provider <vision_model>]`
pub(crate) fn onboard_image(args: &str) -> Result<ToolResult> {
    let args = args.trim();
    if args.is_empty() {
        return Ok(ToolResult::success(IMAGE_MENU.into()));
    }
    let (choice, rest) = split_first(args);
    match choice.to_lowercase().as_str() {
        "gemini" | "google" => {
            let key = rest.trim();
            if key.is_empty() {
                return Ok(ToolResult::error(
                    "Need the key: `/onboard:image gemini <GOOGLE_AI_KEY>`.".into(),
                ));
            }
            if let Err(e) =
                crate::config::write_secret_key("providers.image.gemini", "api_key", key)
            {
                return Ok(ToolResult::error(format!("Failed to save key: {e}")));
            }
            let model = crate::config::default_image_model();
            for (section, k, v) in [
                ("image.vision", "enabled", "true"),
                ("image.vision", "model", model.as_str()),
                ("image.generation", "enabled", "true"),
                ("image.generation", "model", model.as_str()),
            ] {
                if let Err(e) = Config::write_key(section, k, v) {
                    return Ok(ToolResult::error(format!(
                        "Failed to write {section}.{k}: {e}"
                    )));
                }
            }
            Ok(ToolResult::success(
                "Google vision + image generation enabled. analyze_image and generate_image \
                 now use Gemini."
                    .into(),
            ))
        }
        "provider" | "openai" | "openai-compatible" | "compat" => {
            let model = rest.trim();
            if model.is_empty() {
                return Ok(ToolResult::error(
                    "Need the vision model: `/onboard:image provider <VISION_MODEL>` \
                     (a vision-capable model on your active provider)."
                        .into(),
                ));
            }
            let config = match Config::load() {
                Ok(c) => c,
                Err(e) => return Ok(ToolResult::error(format!("Failed to load config: {e}"))),
            };
            let Some(section) = active_provider_section(&config) else {
                return Ok(ToolResult::error(
                    "No active provider. Set one up with /models or /onboard:provider first."
                        .into(),
                ));
            };
            if let Err(e) = Config::write_key(&section, "vision_model", model) {
                return Ok(ToolResult::error(format!(
                    "Failed to write {section}.vision_model: {e}"
                )));
            }
            Ok(ToolResult::success(format!(
                "Vision now routes through your active provider's '{model}' (OpenAI-compatible) — \
                 no extra key needed. analyze_image will use it, with Gemini as fallback if a key \
                 is set."
            )))
        }
        other => Ok(ToolResult::error(format!(
            "Unknown image option '{other}'. Use 'gemini' or 'provider'.\n\n{IMAGE_MENU}"
        ))),
    }
}

const VOICE_MENU: &str = "Voice setup — STT (speech→text) and TTS (text→speech). Pick:\n\
    • `/onboard:voice stt groq <GROQ_KEY>` — Groq Whisper.\n\
    • `/onboard:voice stt openai <BASE_URL> <MODEL> <KEY>` — any OpenAI-compatible STT.\n\
    • `/onboard:voice stt off`\n\
    • `/onboard:voice tts openai <OPENAI_KEY>` — OpenAI TTS.\n\
    • `/onboard:voice tts off`\n\
    Ask the user what they want, then re-run with the argument.";

/// `/onboard:voice [stt … | tts …]`
pub(crate) fn onboard_voice(args: &str) -> Result<ToolResult> {
    let args = args.trim();
    if args.is_empty() {
        return Ok(ToolResult::success(VOICE_MENU.into()));
    }
    let (kind, rest) = split_first(args);
    match kind.to_lowercase().as_str() {
        "stt" => onboard_voice_stt(rest),
        "tts" => onboard_voice_tts(rest),
        other => Ok(ToolResult::error(format!(
            "Use 'stt' or 'tts'. Got '{other}'.\n\n{VOICE_MENU}"
        ))),
    }
}

fn onboard_voice_stt(rest: &str) -> Result<ToolResult> {
    let (provider, params) = split_first(rest);
    match provider.to_lowercase().as_str() {
        "off" => set_flags(&[
            ("providers.stt.openai_compatible", "enabled", "false"),
            ("providers.stt.voicebox", "enabled", "false"),
        ])
        .map(|_| ToolResult::success("STT disabled.".into())),
        "groq" => {
            let key = params.trim();
            if key.is_empty() {
                return Ok(ToolResult::error(
                    "Need the key: stt groq <GROQ_KEY>".into(),
                ));
            }
            if let Err(e) = crate::config::write_secret_key("providers.groq", "api_key", key) {
                return Ok(ToolResult::error(format!("Failed to save key: {e}")));
            }
            Ok(ToolResult::success(
                "Groq Whisper STT enabled (uses your Groq key).".into(),
            ))
        }
        "openai" | "openai-compatible" | "compat" => {
            let parts: Vec<&str> = params.split_whitespace().collect();
            let [base_url, model, key] = parts.as_slice() else {
                return Ok(ToolResult::error(
                    "Usage: stt openai <BASE_URL> <MODEL> <KEY>".into(),
                ));
            };
            set_flags(&[
                ("providers.stt.openai_compatible", "enabled", "true"),
                ("providers.stt.openai_compatible", "base_url", base_url),
                ("providers.stt.openai_compatible", "model", model),
            ])?;
            if let Err(e) =
                crate::config::write_secret_key("providers.stt.openai_compatible", "api_key", key)
            {
                return Ok(ToolResult::error(format!("Failed to save key: {e}")));
            }
            Ok(ToolResult::success(format!(
                "OpenAI-compatible STT enabled ({model} @ {base_url})."
            )))
        }
        other => Ok(ToolResult::error(format!(
            "Unknown STT '{other}'. Use groq, openai, or off."
        ))),
    }
}

fn onboard_voice_tts(rest: &str) -> Result<ToolResult> {
    let (provider, params) = split_first(rest);
    match provider.to_lowercase().as_str() {
        "off" => set_flags(&[("providers.tts.openai", "enabled", "false")])
            .map(|_| ToolResult::success("TTS disabled.".into())),
        "openai" => {
            let key = params.trim();
            if key.is_empty() {
                return Ok(ToolResult::error(
                    "Need the key: tts openai <OPENAI_KEY>".into(),
                ));
            }
            set_flags(&[("providers.tts.openai", "enabled", "true")])?;
            if let Err(e) = crate::config::write_secret_key("providers.openai", "api_key", key) {
                return Ok(ToolResult::error(format!("Failed to save key: {e}")));
            }
            Ok(ToolResult::success("OpenAI TTS enabled.".into()))
        }
        other => Ok(ToolResult::error(format!(
            "Unknown TTS '{other}'. Use openai or off."
        ))),
    }
}

const CHANNELS_MENU: &str = "Channel setup — connect a messenger:\n\
    • `/onboard:channels telegram <BOT_TOKEN>`\n\
    • `/onboard:channels discord <BOT_TOKEN>`\n\
    • `/onboard:channels whatsapp` — starts pairing; I'll send you a QR to scan.\n\
    Ask the user which channel + token, then re-run with the argument.";

/// `/onboard:channels [telegram <token> | discord <token> | whatsapp]`
pub(crate) fn onboard_channels(args: &str) -> Result<ToolResult> {
    let args = args.trim();
    if args.is_empty() {
        return Ok(ToolResult::success(CHANNELS_MENU.into()));
    }
    let (channel, params) = split_first(args);
    match channel.to_lowercase().as_str() {
        "telegram" => {
            let token = params.trim();
            if token.is_empty() {
                return Ok(ToolResult::error(
                    "Need the bot token: `/onboard:channels telegram <BOT_TOKEN>`.".into(),
                ));
            }
            set_flags(&[("channels.telegram", "enabled", "true")])?;
            if let Err(e) = crate::config::write_secret_key("channels.telegram", "token", token) {
                return Ok(ToolResult::error(format!("Failed to save token: {e}")));
            }
            Ok(ToolResult::success(
                "Telegram enabled. Restart (or hot-reload) to start the bot.".into(),
            ))
        }
        "discord" => {
            let token = params.trim();
            if token.is_empty() {
                return Ok(ToolResult::error(
                    "Need the bot token: `/onboard:channels discord <BOT_TOKEN>`.".into(),
                ));
            }
            set_flags(&[("channels.discord", "enabled", "true")])?;
            if let Err(e) = crate::config::write_secret_key("channels.discord", "token", token) {
                return Ok(ToolResult::error(format!("Failed to save token: {e}")));
            }
            Ok(ToolResult::success(
                "Discord enabled. Restart (or hot-reload) to start the bot.".into(),
            ))
        }
        "whatsapp" => Ok(ToolResult::success(
            "WhatsApp pairing uses a QR code. Call the `whatsapp_connect` tool — it starts \
             pairing and yields a QR to scan in WhatsApp → Linked Devices."
                .into(),
        )),
        other => Ok(ToolResult::error(format!(
            "Unknown channel '{other}'. Use telegram, discord, or whatsapp.\n\n{CHANNELS_MENU}"
        ))),
    }
}

/// Write a batch of `config.toml` flags, stopping at the first error.
fn set_flags(flags: &[(&str, &str, &str)]) -> Result<()> {
    for (section, key, val) in flags {
        if let Err(e) = Config::write_key(section, key, val) {
            return Err(super::error::ToolError::Execution(format!(
                "Failed to write {section}.{key}: {e}"
            )));
        }
    }
    Ok(())
}
