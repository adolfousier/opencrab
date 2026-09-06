//! Which voice engine flags a config reload switched OFF (#1399).
//!
//! The owner watched voice disable itself after every restart and the log
//! could not say when or by whom: the watcher logged "reloaded", the
//! channel agent logged the derived STT/TTS booleans, and nothing named the
//! flag that flipped. This diff runs on every reload so the next
//! true-to-false flip leaves a fingerprint with the key that changed and
//! the file's modification time.

use crate::config::Config;

const STT_ENGINES: [&str; 4] = ["groq", "local", "openai_compatible", "voicebox"];
const TTS_ENGINES: [&str; 4] = ["openai", "local", "openai_compatible", "voicebox"];

fn stt_enabled(config: &Config, engine: &str) -> bool {
    let Some(stt) = config.providers.stt.as_ref() else {
        return false;
    };
    match engine {
        "groq" => stt.groq.as_ref().is_some_and(|g| g.enabled),
        "local" => stt.local.as_ref().is_some_and(|l| l.enabled),
        "openai_compatible" => stt.openai_compatible.as_ref().is_some_and(|c| c.enabled),
        "voicebox" => stt.voicebox.as_ref().is_some_and(|v| v.enabled),
        _ => false,
    }
}

fn tts_enabled(config: &Config, engine: &str) -> bool {
    let Some(tts) = config.providers.tts.as_ref() else {
        return false;
    };
    match engine {
        "openai" => tts.openai.as_ref().is_some_and(|o| o.enabled),
        "local" => tts.local.as_ref().is_some_and(|l| l.enabled),
        "openai_compatible" => tts.openai_compatible.as_ref().is_some_and(|c| c.enabled),
        "voicebox" => tts.voicebox.as_ref().is_some_and(|v| v.enabled),
        _ => false,
    }
}

/// Dotted config keys whose `enabled` went from true in `prev` to false in
/// `next`, e.g. `providers.tts.openai.enabled`. Flags that turned on are
/// not reported: switching voice on is what the user asked for, switching
/// it off behind their back is the incident.
pub fn voice_flags_switched_off(prev: &Config, next: &Config) -> Vec<String> {
    let mut off = Vec::new();
    for engine in STT_ENGINES {
        if stt_enabled(prev, engine) && !stt_enabled(next, engine) {
            off.push(format!("providers.stt.{engine}.enabled"));
        }
    }
    for engine in TTS_ENGINES {
        if tts_enabled(prev, engine) && !tts_enabled(next, engine) {
            off.push(format!("providers.tts.{engine}.enabled"));
        }
    }
    off
}
