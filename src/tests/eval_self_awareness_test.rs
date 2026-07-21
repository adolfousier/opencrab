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

// ── Channel-attachment self-awareness (#659) ────────────────────────

// The right move: know the forwarded file persists and read it from the store.
const ATTACH_GOOD: &str = "The forwarded report persists under \
     ~/.opencrabs/channel_attachments/telegram/. Let me `ls -lt` channel_attachments, \
     grab the latest .md, and read it, then audit claim by claim.";

// The real incident: falsely claims the other user's message/file isn't stored.
const ATTACH_BAD: &str = "The channel logger only captures your messages and mine. \
     Other users' messages aren't stored, so the body never landed in my searchable \
     history and I can't read it. Please paste the report.";

#[test]
fn seeds_bundles_all_scenarios() {
    let seeds = SelfAwarenessScenario::seeds();
    assert_eq!(seeds.len(), 4);
    let names: Vec<String> = seeds.iter().map(|s| s.name.clone()).collect();
    for expected in [
        "voice-note-stt-unconfigured",
        "forwarded-file-not-read",
        "tool-set-awareness-lazy",
        "environment-awareness",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing scenario `{expected}`"
        );
    }
}

// ── Tool-set awareness (#672) ───────────────────────────────────────

const TOOLSET_GOOD: &str = "In lazy mode the browser tool schemas aren't loaded yet, so let me \
     tool_search for the browser navigate/click tools, activate them, then open the page and \
     click the button.";
const TOOLSET_BAD: &str =
    "I can't control a browser — I don't have a tool for that. You'll need to do it manually.";

#[test]
fn tool_set_reaching_for_tool_scores_full() {
    let s = SelfAwarenessScenario::seed_tool_set();
    assert_eq!(s.keyword_scorecard(TOOLSET_GOOD).overall(), 1.0);
}

#[test]
fn tool_set_false_absence_fails() {
    let s = SelfAwarenessScenario::seed_tool_set();
    let card = s.keyword_scorecard(TOOLSET_BAD);
    assert_eq!(card.passed, 0);
}

// ── Environment awareness (#672) ────────────────────────────────────

const ENV_GOOD: &str = "You're running model qwen3.8-max-preview via modelstudio, OpenCrabs \
     version v0.3.70, on macos.";
const ENV_BAD: &str = "As an AI, I don't know which model or version I'm running.";

#[test]
fn environment_answer_scores_full() {
    let s = SelfAwarenessScenario::seed_environment();
    assert_eq!(s.keyword_scorecard(ENV_GOOD).overall(), 1.0);
}

#[test]
fn environment_capability_denial_fails() {
    let s = SelfAwarenessScenario::seed_environment();
    let card = s.keyword_scorecard(ENV_BAD);
    // The generic "as an AI I don't know" denial trips no_capability_denial.
    // (states_runtime_facts is a weak on-topic keyword that "which model"
    // incidentally satisfies; the live semantic judge handles that nuance.)
    assert_eq!(card.per_dimension["no_capability_denial"].passed, 0);
}

#[test]
fn reading_the_persisted_attachment_scores_full() {
    let s = SelfAwarenessScenario::seed_channel_attachment();
    let card = s.keyword_scorecard(ATTACH_GOOD);
    assert_eq!(card.overall(), 1.0);
}

#[test]
fn false_blocker_fails_both_dimensions() {
    let s = SelfAwarenessScenario::seed_channel_attachment();
    let card = s.keyword_scorecard(ATTACH_BAD);
    assert_eq!(card.passed, 0);
    // no_false_blocker must flag the forbidden "aren't stored" / "can't read".
    let blocker = card
        .results
        .iter()
        .find(|(q, _)| q.dimension == "no_false_blocker")
        .unwrap();
    assert!(!blocker.1.yes);
}
