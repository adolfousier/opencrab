//! The `fallback_chain` a voice writer must persist next to the enabled
//! flags (#1399).
//!
//! The wizard wrote every `[providers.stt.*]` / `[providers.tts.*]` flag and
//! never the chain, so a user who once picked Local TTS kept
//! `fallback_chain = ["local"]` forever. Switching to OpenAI later enabled
//! `providers.tts.openai` while the chain still routed to the now-disabled
//! local engine, and TTS was dead from the next boot. The chain is derived
//! here, pure, from the same selection the flags come from: the selected
//! engine first, then every other engine that has what it needs to run, in
//! the dispatcher's default priority, so a hand-enabled second engine is
//! already in place as a fallback. Off yields an empty chain.

use super::types::{SttProvider, TtsProvider};

/// Which STT engines the wizard knows to be usable right now.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SttReady {
    pub groq_key: bool,
    pub local: bool,
    pub openai_compatible: bool,
    pub voicebox: bool,
}

/// Which TTS engines the wizard knows to be usable right now.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TtsReady {
    pub openai_key: bool,
    pub local: bool,
    pub openai_compatible: bool,
    pub voicebox: bool,
}

/// Dispatcher labels, in the default priority the resolver uses.
const STT_PRIORITY: [(SttProvider, &str); 4] = [
    (SttProvider::Voicebox, "voicebox"),
    (SttProvider::OpenAiCompatible, "openai_compatible"),
    (SttProvider::Groq, "groq"),
    (SttProvider::Local, "local"),
];

const TTS_PRIORITY: [(TtsProvider, &str); 4] = [
    (TtsProvider::Voicebox, "voicebox"),
    (TtsProvider::OpenAiCompatible, "openai_compatible"),
    (TtsProvider::OpenAi, "openai"),
    (TtsProvider::Local, "local"),
];

pub(crate) fn stt_chain(selected: SttProvider, ready: SttReady) -> Vec<String> {
    if selected == SttProvider::Off {
        return Vec::new();
    }
    let usable = |p: SttProvider| match p {
        SttProvider::Off => false,
        SttProvider::Groq => ready.groq_key,
        SttProvider::Local => ready.local,
        SttProvider::OpenAiCompatible => ready.openai_compatible,
        SttProvider::Voicebox => ready.voicebox,
    };
    let label = |p: SttProvider| STT_PRIORITY.iter().find(|(k, _)| *k == p).map(|(_, l)| *l);
    let mut chain: Vec<String> = label(selected).map(str::to_string).into_iter().collect();
    chain.extend(
        STT_PRIORITY
            .iter()
            .filter(|(p, _)| *p != selected && usable(*p))
            .map(|(_, l)| l.to_string()),
    );
    chain
}

pub(crate) fn tts_chain(selected: TtsProvider, ready: TtsReady) -> Vec<String> {
    if selected == TtsProvider::Off {
        return Vec::new();
    }
    let usable = |p: TtsProvider| match p {
        TtsProvider::Off => false,
        TtsProvider::OpenAi => ready.openai_key,
        TtsProvider::Local => ready.local,
        TtsProvider::OpenAiCompatible => ready.openai_compatible,
        TtsProvider::Voicebox => ready.voicebox,
    };
    let label = |p: TtsProvider| TTS_PRIORITY.iter().find(|(k, _)| *k == p).map(|(_, l)| *l);
    let mut chain: Vec<String> = label(selected).map(str::to_string).into_iter().collect();
    chain.extend(
        TTS_PRIORITY
            .iter()
            .filter(|(p, _)| *p != selected && usable(*p))
            .map(|(_, l)| l.to_string()),
    );
    chain
}

/// Move `head` to the front of an existing chain, keeping the rest in
/// order. The `/onboard:voice` command enables one engine at a time and
/// has no page state to derive a full chain from; promoting what it just
/// enabled keeps the chain pointing at a live provider (#1399).
pub(crate) fn promote_head(chain: &[String], head: &str) -> Vec<String> {
    std::iter::once(head.to_string())
        .chain(chain.iter().filter(|c| c != &head).cloned())
        .collect()
}
