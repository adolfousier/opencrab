//! Wiring tests for the #1240 stale-claim scan inside the RSI cycle
//! (`src/brain/rsi.rs` — the task-5 half of the RFC).
//!
//! Two properties, straight from the plan's acceptance criteria:
//!
//! 1. **Dataflow** — a real gated scan's output provably reaches the
//!    improvement-step prompt: `run_scan_with_ledger` →
//!    [`StaleScanInput::from_outcome`] → [`stale_scan_prompt_block`] →
//!    [`build_cycle_prompt`], with the findings (file, line, anchor,
//!    evidence) and the `self_improve action='update'` recommendation all
//!    present in the resulting prompt bytes. A source pin additionally
//!    proves `run_rsi_agent_cycle` itself derives its prompt from the scan
//!    ahead of the `send_message_with_tools` improvement step.
//! 2. **Byte-identical fast path** — when the scan is disabled by the
//!    cadence gate (Skip), or a fresh instance has nothing stale to flag,
//!    the cycle prompt is byte-for-byte the pre-#1240 construction
//!    (golden replica below), so most cycles pay zero observable cost.
//!
//! Plus the no-spam property at the wire level: a finding reaches the
//! prompt exactly once (ledger dedup) — later cycles map to the empty
//! input again.

use crate::brain::rsi::{StaleScanInput, build_cycle_prompt, stale_scan_prompt_block};
use crate::brain::rsi_stale_ledger::{ScanRunOutcome, run_scan_with_ledger};
use crate::config::Config;
use std::sync::atomic::{AtomicUsize, Ordering};

const RSI_SRC: &str = include_str!("../brain/rsi.rs");

// ------------------------------------------------------------------ helpers

/// Unique scratch dir per call: parallel test files must never share a
/// tempdir (same hazard the scan/ledger suites guard with their atomics).
fn scratch_dir(tag: &str) -> std::path::PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "oc-rsi-stale-wire-{}-{n}-{tag}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// The pre-#1240 prompt construction, replicated VERBATIM from the last
/// pre-change `run_rsi_agent_cycle` as the golden baseline. Any byte the
/// new `build_cycle_prompt(_, "")` emits differently fails the
/// byte-identity tests below.
fn pre_change_baseline(opportunities: &[String]) -> String {
    let mut prompt = "Run an autonomous self-improvement cycle.\n\n".to_string();
    if !opportunities.is_empty() {
        prompt.push_str("Detected opportunities:\n");
        for (i, opp) in opportunities.iter().enumerate() {
            prompt.push_str(&format!("{}. {opp}\n", i + 1));
        }
        prompt.push('\n');
    }
    prompt.push_str(
        "Analyze the feedback data, identify the highest-impact issues, and apply improvements.\n",
    );
    prompt.push_str(&crate::brain::rsi_disposition::required_actions_block(
        opportunities,
    ));
    prompt
}

/// A brain with one stale finding of each wire-relevant flavor.
///
/// NOTE: prescription lines here must avoid 4-digit runs starting with 1
/// or 2 (`has_date_marker` would exempt them as years) and the
/// historical-hint words — the scan test suite pins that behavior.
fn stale_brain(dir: &std::path::Path) {
    std::fs::write(
        dir.join("SOUL.md"),
        "# soul\n\n- run `oc-wire-dead-bin` every morning\n\n- check \
         `/opt/oc-wire-gone-path/rule.md` before deploy\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("AGENTS.md"),
        "# agents\n\n- use provider `oc-wire-no-provider` for image work\n\n- use `cd` when \
         moving between directories\n",
    )
    .unwrap();
}

fn day(secs: u64) -> u64 {
    1_700_000_000u64 + secs
}

/// The full wire chain as run_rsi_agent_cycle composes it.
fn cycle_prompt_from_scan(
    config: &Config,
    brain: &std::path::Path,
    ledger: &std::path::Path,
    now_unix: u64,
    cycle: u64,
    binary_version: &str,
    opportunities: &[String],
) -> (String, StaleScanInput) {
    let outcome = run_scan_with_ledger(config, brain, ledger, now_unix, cycle, binary_version);
    let input = StaleScanInput::from_outcome(&outcome);
    let prompt = build_cycle_prompt(opportunities, &stale_scan_prompt_block(&input));
    (prompt, input)
}

// ------------------------------------------------ acceptance criterion 1
// dataflow: scan output reaches the improvement-step prompt struct

#[test]
fn scan_findings_reach_the_cycle_prompt_structured() {
    let dir = scratch_dir("dataflow");
    let brain = dir.join("brain");
    std::fs::create_dir_all(&brain).unwrap();
    stale_brain(&brain);
    let ledger = dir.join("rsi/stale_scan.json");
    let opps = vec!["tool bash failure rate 30% over 7d".to_string()];

    let (prompt, input) = cycle_prompt_from_scan(
        &Config::default(),
        &brain,
        &ledger,
        day(0),
        1,
        "0.3.83",
        &opps,
    );

    // The scan's actionable set is partitioned along the RFC action matrix:
    // dead binary + unconfigured provider → reword via update; vanished
    // path → owner sign-off, never agent-edited.
    assert_eq!(input.reword_via_update.len(), 2, "{input:?}");
    assert_eq!(input.owner_signoff.len(), 1, "{input:?}");
    assert!(input.cleared.is_empty());

    // Structured provenance from the scan output is IN the prompt bytes:
    // file:line, the dead anchor, and the verification evidence.
    assert!(prompt.contains("1. SOUL.md:3\n   line: - run `oc-wire-dead-bin` every morning\n   dead anchor: binary `oc-wire-dead-bin` — no executable on PATH\n"), "{prompt}");
    assert!(
        prompt.contains("2. AGENTS.md:3\n   line: - use provider `oc-wire-no-provider` for image work\n   dead anchor: provider `oc-wire-no-provider`"),
        "{prompt}"
    );
    assert!(
        prompt.contains("- SOUL.md:5\n  line: - check `/opt/oc-wire-gone-path/rule.md` before deploy\n  dead anchor: path `/opt/oc-wire-gone-path/rule.md` — path does not exist\n"),
        "{prompt}"
    );

    // The recommendation is the RFC's verb of record.
    assert!(
        prompt.contains("self_improve action='update'"),
        "the actionable findings must recommend self_improve action='update'"
    );

    // Owner-sign-off items are explicitly NOT agent-editable (no silent
    // deletes, no autonomous removals — RFC design decision 2).
    assert!(prompt.contains("OWNER SIGN-OFF REQUIRED"));
    assert!(prompt.contains("do NOT edit these yourself"));

    // The healthy control (`cd`, a shell builtin) never appears.
    assert!(!prompt.contains("`cd`"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stale_block_precedes_capability_gap_actions() {
    // #842 pin: capability gaps must still CLOSE the prompt. The stale
    // block is inserted before required_actions_block, so a cycle with
    // both keeps the #842 ordering.
    let dir = scratch_dir("order");
    let brain = dir.join("brain");
    std::fs::create_dir_all(&brain).unwrap();
    stale_brain(&brain);
    let ledger = dir.join("rsi/stale_scan.json");
    let opps = vec![
        "Tool sequence 'a -> b' ran in 5 sessions — candidate for a skill (rsi_propose kind=skill). File a SKILL.md.".to_string(),
    ];

    let (prompt, _) = cycle_prompt_from_scan(
        &Config::default(),
        &brain,
        &ledger,
        day(0),
        1,
        "0.3.83",
        &opps,
    );

    let stale_at = prompt
        .find("STALE-CLAIM SCAN FINDINGS")
        .expect("stale block");
    let actions_at = prompt.find("REQUIRED ACTIONS").expect("capability block");
    assert!(
        stale_at < actions_at,
        "capability-gap actions must close the prompt (#842)"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[cfg(unix)] // probe path is a literal /tmp path; verify_path follows symlinks (/tmp → /private/tmp on macOS)
fn cleared_findings_flow_through_as_informational() {
    // A stale path that later verifies Ok again must reach the prompt as a
    // cleared note (never a silent drop) — the ledger's flip semantics
    // surfaced at the wire.
    let probe = std::path::Path::new("/tmp/oc-rsi-wire-cleared-probe");
    let _ = std::fs::remove_dir_all(probe);

    let dir = scratch_dir("cleared");
    let brain = dir.join("brain");
    std::fs::create_dir_all(&brain).unwrap();
    std::fs::write(
        brain.join("AGENTS.md"),
        "# agents\n\n- check `/tmp/oc-rsi-wire-cleared-probe/rule.md` before deploy\n",
    )
    .unwrap();
    let ledger = dir.join("rsi/stale_scan.json");
    let cfg = Config::default();
    let opps = vec!["opportunity".to_string()];

    // Run 1: probe path missing → stale (owner bucket), announced once.
    let (_, first) = cycle_prompt_from_scan(&cfg, &brain, &ledger, day(0), 1, "0.3.83", &opps);
    assert_eq!(first.owner_signoff.len(), 1, "{first:?}");

    // The path comes back into existence; binary version also changes,
    // which force-runs the scan past the daily gate.
    std::fs::create_dir_all(probe).unwrap();
    std::fs::write(probe.join("rule.md"), "back\n").unwrap();
    let (prompt, second) =
        cycle_prompt_from_scan(&cfg, &brain, &ledger, day(60), 2, "0.3.84", &opps);
    assert!(second.reword_via_update.is_empty());
    assert!(second.owner_signoff.is_empty());
    assert_eq!(second.cleared.len(), 1, "stale→ok surfaces as cleared");
    assert!(
        prompt.contains("PREVIOUSLY STALE, NOW HEALTHY AGAIN"),
        "{prompt}"
    );
    assert!(
        prompt.contains("- AGENTS.md:3 — `/tmp/oc-rsi-wire-cleared-probe/rule.md` verified ok"),
        "{prompt}"
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(probe);
}

// ------------------------------------------------ acceptance criterion 2
// disabled / fresh-instance path is byte-identical to the pre-change
// baseline (empty-scan fast path)

#[test]
fn empty_block_is_byte_identical_to_pre_change_baseline() {
    // Every opportunity shape the cycle builds prompts for: empty,
    // guidance-only, and a capability gap (which appends the
    // required-actions tail).
    let shapes: Vec<Vec<String>> = vec![
        vec![],
        vec!["tool bash failure rate 30% over 7d".to_string()],
        vec![
            "Command pattern 'git status' recurring — candidate (rsi_propose kind=command)."
                .to_string(),
            "Tool sequence 'a -> b' ran in 5 sessions (rsi_propose kind=skill).".to_string(),
        ],
    ];
    for opps in &shapes {
        assert_eq!(
            build_cycle_prompt(opps, ""),
            pre_change_baseline(opps),
            "empty stale block must leave the prompt byte-identical (shape: {opps:?})"
        );
    }
    // The default (empty) input renders as the empty block directly.
    assert_eq!(stale_scan_prompt_block(&StaleScanInput::default()), "");
}

#[test]
fn cadence_disabled_path_is_byte_identical() {
    // "Disabled" at the cycle level = the cadence gate says Skip (scan
    // already ran within 24h on this binary). The cycle must observe ZERO
    // difference from the pre-#1240 baseline.
    let dir = scratch_dir("skip");
    let brain = dir.join("brain");
    std::fs::create_dir_all(&brain).unwrap();
    stale_brain(&brain); // findings EXIST — the gate must still hide them
    let ledger = dir.join("rsi/stale_scan.json");
    let cfg = Config::default();
    let opps = vec!["opportunity".to_string()];

    // First run primes the ledger (findings surface here — that is the
    // feature, tested above).
    let (first_prompt, first) =
        cycle_prompt_from_scan(&cfg, &brain, &ledger, day(0), 1, "0.3.83", &opps);
    assert!(!first.is_empty());
    assert!(first_prompt.contains("STALE-CLAIM SCAN FINDINGS"));

    // One minute later, same binary: the gate skips, byte-identical.
    let outcome = run_scan_with_ledger(&cfg, &brain, &ledger, day(0) + 60, 2, "0.3.83");
    assert!(matches!(outcome, ScanRunOutcome::Skipped { .. }));
    let input = StaleScanInput::from_outcome(&outcome);
    assert!(input.is_empty());
    let prompt = build_cycle_prompt(&opps, &stale_scan_prompt_block(&input));
    assert_eq!(prompt, pre_change_baseline(&opps));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fresh_instance_with_nothing_stale_is_byte_identical() {
    // Fresh install semantics: no ledger yet (first run), brain files
    // carry no stale anchors → scan runs, flags nothing → the cycle is
    // indistinguishable from the pre-#1240 baseline.
    let dir = scratch_dir("fresh");
    let brain = dir.join("brain");
    std::fs::create_dir_all(&brain).unwrap();
    std::fs::write(
        brain.join("SOUL.md"),
        "# soul\n\n- use `cd` when moving between directories\n",
    )
    .unwrap();
    let ledger = dir.join("rsi/stale_scan.json");
    let opps = vec!["opportunity".to_string()];

    let (prompt, input) = cycle_prompt_from_scan(
        &Config::default(),
        &brain,
        &ledger,
        day(0),
        1,
        "0.3.83",
        &opps,
    );
    assert!(
        input.is_empty(),
        "healthy anchors surface nothing: {input:?}"
    );
    assert_eq!(prompt, pre_change_baseline(&opps));

    // Truly empty brain dir (pre-template first boot): also nothing.
    let dir2 = scratch_dir("fresh-empty");
    let brain2 = dir2.join("brain");
    std::fs::create_dir_all(&brain2).unwrap();
    let (prompt2, input2) = cycle_prompt_from_scan(
        &Config::default(),
        &brain2,
        &dir2.join("rsi/stale_scan.json"),
        day(0),
        1,
        "0.3.83",
        &opps,
    );
    assert!(input2.is_empty());
    assert_eq!(prompt2, pre_change_baseline(&opps));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

// -------------------------------------------------------- no-spam property

#[test]
fn a_finding_feeds_exactly_one_cycle_prompt() {
    // The inbox-noise guarantee at the wire: the same stale finding is fed
    // to the improvement step ONCE. Within the cadence window the gate
    // skips; past it, the ledger's dedup suppresses the same-verdict
    // repeat — every later cycle maps back to the byte-identical baseline.
    let dir = scratch_dir("nospam");
    let brain = dir.join("brain");
    std::fs::create_dir_all(&brain).unwrap();
    stale_brain(&brain);
    let ledger = dir.join("rsi/stale_scan.json");
    let cfg = Config::default();
    let opps = vec!["opportunity".to_string()];

    // Cycle 1: findings feed the prompt once.
    let (p1, i1) = cycle_prompt_from_scan(&cfg, &brain, &ledger, day(0), 1, "0.3.83", &opps);
    assert!(!i1.is_empty() && p1.contains("STALE-CLAIM SCAN FINDINGS"));

    // Cycle 2 (next day, same binary): scan runs, dedup suppresses every
    // repeat — prompt back to baseline bytes.
    let (p2, i2) = cycle_prompt_from_scan(
        &cfg,
        &brain,
        &ledger,
        day(0) + 25 * 3600,
        2,
        "0.3.83",
        &opps,
    );
    assert!(i2.is_empty(), "no re-flag on identical findings: {i2:?}");
    assert_eq!(p2, pre_change_baseline(&opps));

    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------- source pin

#[test]
fn run_rsi_agent_cycle_wires_scan_ahead_of_the_improvement_step() {
    // Proves the dataflow at the source level: the cycle function itself
    // (a) runs the gated scan, (b) builds its prompt from the scan's block
    // via build_cycle_prompt, and (c) only then reaches the improvement
    // step (send_message_with_tools) — in that order.
    let cycle_start = RSI_SRC
        .find("async fn run_rsi_agent_cycle(")
        .expect("run_rsi_agent_cycle defined");
    let after = &RSI_SRC[cycle_start..];
    // All offsets below are relative to `scan_at` (a common base), so the
    // ordering comparison is apples-to-apples.
    let scan_at = after
        .find("stale_scan_cycle_input(config)")
        .expect("cycle runs the gated stale scan");
    let after_scan = &after[scan_at..];
    let block_at = after_scan
        .find("stale_scan_prompt_block(&stale_input)")
        .expect("cycle renders the scan block into the prompt");
    let build_at = after_scan
        .find("build_cycle_prompt(")
        .expect("cycle builds its prompt via build_cycle_prompt");
    let send_at = after_scan
        .find(".send_message_with_tools(")
        .expect("improvement step call");
    // Note on order: `build_cycle_prompt(opportunities,
    // &stale_scan_prompt_block(&stale_input))` puts the builder name
    // textually before the block expression it wraps, so build_at <
    // block_at is expected; what must hold is that BOTH sit between the
    // scan and the improvement step.
    assert!(
        build_at < send_at && block_at < send_at,
        "scan → block → prompt → send ordering must hold inside run_rsi_agent_cycle"
    );
}
