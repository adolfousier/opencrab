//! The fallback chain a voice writer persists with the flags (#1399).

use crate::tui::onboarding::voice_chain::{SttReady, TtsReady, promote_head, stt_chain, tts_chain};
use crate::tui::onboarding::{SttProvider, TtsProvider};

#[test]
fn off_yields_an_empty_chain() {
    assert!(
        stt_chain(
            SttProvider::Off,
            SttReady {
                groq_key: true,
                ..Default::default()
            }
        )
        .is_empty()
    );
    assert!(
        tts_chain(
            TtsProvider::Off,
            TtsReady {
                openai_key: true,
                ..Default::default()
            }
        )
        .is_empty()
    );
}

#[test]
fn selected_engine_leads_the_chain() {
    let chain = tts_chain(
        TtsProvider::OpenAi,
        TtsReady {
            openai_key: true,
            local: true,
            ..Default::default()
        },
    );
    assert_eq!(chain, vec!["openai", "local"]);
}

#[test]
fn selected_engine_leads_even_when_it_is_last_in_default_priority() {
    let chain = tts_chain(
        TtsProvider::Local,
        TtsReady {
            openai_key: true,
            local: true,
            voicebox: true,
            ..Default::default()
        },
    );
    assert_eq!(chain, vec!["local", "voicebox", "openai"]);
}

#[test]
fn engines_without_config_stay_out_of_the_chain() {
    let chain = stt_chain(
        SttProvider::Groq,
        SttReady {
            groq_key: true,
            ..Default::default()
        },
    );
    assert_eq!(chain, vec!["groq"]);
}

#[test]
fn other_engines_follow_in_dispatcher_priority() {
    let chain = stt_chain(
        SttProvider::Groq,
        SttReady {
            groq_key: true,
            local: true,
            openai_compatible: true,
            voicebox: true,
        },
    );
    assert_eq!(
        chain,
        vec!["groq", "voicebox", "openai_compatible", "local"]
    );
}

#[test]
fn a_selected_engine_leads_even_when_its_own_readiness_is_unknown() {
    // The flags say it is enabled; the chain must agree with the flags.
    let chain = tts_chain(TtsProvider::OpenAi, TtsReady::default());
    assert_eq!(chain, vec!["openai"]);
}

#[test]
fn promote_head_moves_the_engine_to_the_front_without_duplicates() {
    let chain: Vec<String> = vec!["local".into(), "openai".into()];
    assert_eq!(promote_head(&chain, "openai"), vec!["openai", "local"]);
    assert_eq!(promote_head(&[], "groq"), vec!["groq"]);
    assert_eq!(promote_head(&chain, "local"), vec!["local", "openai"]);
}
