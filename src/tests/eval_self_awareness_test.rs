//! Tests for the capability self-awareness eval (#636).

use crate::eval::replay::ReplayProvider;
use crate::eval::scorer::ProviderJudge;
use crate::eval::self_awareness::SelfAwarenessScenario;

const GOOD: &str = "OpenCrabs has local-stt compiled in (offline, no key). I'll enable it via \
     config_manager / /onboard so voice notes transcribe automatically — no external code needed.";

const BAD: &str = "I'll build a Python codebase with Whisper to transcribe the audio. \
     Run `pip install openai-whisper`, then `def transcribe(path): ...`.";

#[test]
fn seed_scenario_loads() {
    let s = SelfAwarenessScenario::seed();
    assert_eq!(s.name, "voice-note-stt-unconfigured");
    // Two awareness dimensions: reaches for config tooling, and does not
    // reimplement. (uses_builtin completion probe dropped in #644.)
    assert_eq!(s.probes.len(), 2);
}

#[test]
fn configuring_response_scores_full() {
    let s = SelfAwarenessScenario::seed();
    let card = s.keyword_scorecard(GOOD);
    assert_eq!(card.overall(), 1.0);
    assert_eq!(card.per_dimension.len(), 2);
}

#[test]
fn reimplementing_response_fails_every_dimension() {
    let s = SelfAwarenessScenario::seed();
    let card = s.keyword_scorecard(BAD);
    assert_eq!(card.passed, 0);
    // The no-reimplement probe explains which forbidden signal it hit.
    let noreimpl = card
        .results
        .iter()
        .find(|(q, _)| q.dimension == "no_reimplement")
        .unwrap();
    assert!(
        noreimpl
            .1
            .explanation
            .as_deref()
            .unwrap()
            .contains("forbidden")
    );
}

#[test]
fn forbidden_keyword_alone_fails_the_probe() {
    let s = SelfAwarenessScenario::seed();
    // Mentions the built-in AND config, but still proposes pip install.
    let mixed = "I'll enable local-stt via config, but also pip install whisper as a backup.";
    let card = s.keyword_scorecard(mixed);
    // self_configures passes; no_reimplement fails on the forbidden term.
    assert_eq!(card.passed, 1);
    assert_eq!(card.per_dimension["no_reimplement"].passed, 0);
}

#[tokio::test]
async fn judge_path_grades_response_offline() {
    let s = SelfAwarenessScenario::seed();
    // Scripted judge: YES, YES for the 2 probes.
    let provider =
        ReplayProvider::from_json(r#"{"model":"j","turns":[{"text":"YES"},{"text":"YES"}]}"#)
            .unwrap();
    let judge = ProviderJudge::new(&provider, "j");
    let card = s.judge_scorecard(&judge, GOOD).await;
    assert_eq!((card.passed, card.total), (2, 2));
    assert_eq!(provider.turns_consumed(), 2);
}
