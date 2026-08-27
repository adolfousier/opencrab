//! Tests for the stale-scan ledger, dedup, and cadence gate (#1240 task 4,
//! `src/brain/rsi_stale_ledger.rs`).
//!
//! Coverage mirrors the plan's acceptance criteria for the persistence half
//! of the RFC:
//! 1. **second identical scan reports zero new flags** — same-verdict
//!    repeats are suppressed after the first surfacing;
//! 2. **cadence logic covered** — the pure gate is tested branch-by-branch
//!    (first run, daily elapsed, within-window skip, binary-version force);
//! 3. **ledger survives a simulated restart** — save → fresh `load` from a
//!    different handle reproduces it byte-for-semantics;
//!
//! Plus the honesty edges: verdict CHANGES re-surface in their direction,
//! `Unverifiable` flips never announce, and corrupt/foreign ledgers degrade
//! to first-run instead of erroring.

use crate::brain::rsi_stale_ledger::{
    CadenceDecision, LEDGER_SCHEMA_VERSION, SCAN_MIN_INTERVAL_SECS, StaleScanLedger, cadence_gate,
    default_ledger_path, diff_and_record, ledger_key, rule_hash, run_scan_with_ledger,
};
use crate::brain::rsi_stale_scan::{AnchorKind, FindingAction, StaleFinding, Verdict};
use crate::config::Config;
use std::sync::atomic::{AtomicUsize, Ordering};

// ------------------------------------------------------------------ helpers

/// Unique scratch dir per call: parallel test files must never share a
/// tempdir (same hazard the scan suite guards with its atomic SEQ).
fn scratch_dir(tag: &str) -> std::path::PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "oc-rsi-stale-ledger-{}-{}-{tag}",
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn finding(
    file: &str,
    line_no: usize,
    line: &str,
    verdict: Verdict,
    evidence: &str,
) -> StaleFinding {
    StaleFinding {
        file: file.to_string(),
        line_no,
        line: line.to_string(),
        anchor: format!("anchor-{line_no}"),
        anchor_kind: AnchorKind::Binary,
        verdict,
        action: if verdict == Verdict::Stale {
            FindingAction::RewordViaUpdate
        } else {
            FindingAction::None
        },
        evidence: evidence.to_string(),
    }
}

fn day(secs: u64) -> u64 {
    1_700_000_000u64 + secs
}

// ------------------------------------------------------------- cadence gate

#[test]
fn cadence_first_run_without_ledger() {
    let d = cadence_gate(None, day(0), "0.3.83");
    assert_eq!(
        d,
        CadenceDecision::Run {
            forced: false,
            reason: "first run: no ledger on disk"
        }
    );
}

#[test]
fn cadence_binary_change_forces_immediate_run() {
    let ledger = StaleScanLedger {
        binary_version: "0.3.83".into(),
        last_run_unix: day(9),
        ..Default::default()
    };
    let d = cadence_gate(Some(&ledger), day(60), "0.3.84");
    assert_eq!(
        d,
        CadenceDecision::Run {
            forced: true,
            reason: "binary version changed since last ledger write"
        }
    );
}

#[test]
fn cadence_within_window_same_binary_skips() {
    let ledger = StaleScanLedger {
        binary_version: "0.3.83".into(),
        last_run_unix: day(100),
        ..Default::default()
    };
    let d = cadence_gate(Some(&ledger), day(100) + 3600, "0.3.83");
    assert!(matches!(d, CadenceDecision::Skip { .. }));
}

#[test]
fn cadence_daily_interval_elapsed_runs() {
    let ledger = StaleScanLedger {
        binary_version: "0.3.83".into(),
        last_run_unix: day(0),
        ..Default::default()
    };
    let d = cadence_gate(Some(&ledger), day(0) + SCAN_MIN_INTERVAL_SECS, "0.3.83");
    assert!(matches!(d, CadenceDecision::Run { forced: false, .. }));
}

// ------------------------------------------------------------------- hashes

#[test]
fn rule_hash_is_whitespace_trim_stable() {
    assert_eq!(
        rule_hash("  use `omdc-proxy` daily  "),
        rule_hash("use `omdc-proxy` daily")
    );
    assert_ne!(rule_hash("rule a"), rule_hash("rule b"));
}

#[test]
fn ledger_key_pairs_hash_with_anchor() {
    let h = rule_hash("some rule");
    assert_eq!(ledger_key(&h, "/usr/bin/x"), format!("{h}:/usr/bin/x"));
}

// ------------------------------------------- persistence / restart survival

#[test]
fn ledger_survives_simulated_restart_roundtrip() {
    let dir = scratch_dir("roundtrip");
    let path = dir.join("rsi/stale_scan.json");

    let mut entries = std::collections::BTreeMap::new();
    entries.insert(
        ledger_key(&rule_hash("dead rule"), "/usr/bin/gone"),
        crate::brain::rsi_stale_ledger::LedgerEntry {
            verdict: Verdict::Stale,
            evidence: "no such binary on PATH".into(),
            last_verified_cycle: 7,
            outstanding_stale: true,
        },
    );
    let original = StaleScanLedger {
        schema_version: LEDGER_SCHEMA_VERSION,
        binary_version: "0.3.83".into(),
        last_run_unix: day(5),
        entries,
    };
    original.save(&path).expect("save ledger");

    // Simulated restart: brand-new handle, cold read from disk.
    let revived = StaleScanLedger::load(&path).expect("ledger readable after restart");
    assert_eq!(revived, original);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ledger_load_missing_corrupt_or_foreign_schema_degrades_to_none() {
    let dir = scratch_dir("degrade");
    // missing
    assert!(StaleScanLedger::load(&dir.join("absent.json")).is_none());
    // corrupt JSON
    let bad = dir.join("bad.json");
    std::fs::write(&bad, "{ not json ").unwrap();
    assert!(StaleScanLedger::load(&bad).is_none());
    // valid JSON, foreign schema version
    let foreign = dir.join("foreign.json");
    std::fs::write(&foreign, format!("{{\"schema_version\":{},\"binary_version\":\"\",\"last_run_unix\":0,\"entries\":{{}}}}", LEDGER_SCHEMA_VERSION + 1)).unwrap();
    assert!(StaleScanLedger::load(&foreign).is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------ dedup / diff

#[test]
fn second_identical_scan_reports_zero_new_flags() {
    let fs_scan = || {
        vec![finding(
            "AGENTS.md",
            12,
            "run `omdc-proxy` nightly",
            Verdict::Stale,
            "not on PATH",
        )]
    };

    let first = diff_and_record(fs_scan(), StaleScanLedger::default(), day(0), 1, "0.3.83");
    assert_eq!(first.to_surface.len(), 1, "first sighting surfaces once");
    assert_eq!(first.cleared.len(), 0);
    assert_eq!(first.suppressed, 0);

    // Identical re-scan on the updated ledger: flag is remembered, not repeated.
    let second = diff_and_record(fs_scan(), first.ledger, day(1), 2, "0.3.83");
    assert!(
        second.to_surface.is_empty(),
        "zero new flags on identical re-run"
    );
    assert_eq!(second.suppressed, 1);
    assert_eq!(second.cleared.len(), 0);
}

#[test]
fn first_sighting_ok_is_bookkeeping_not_news() {
    let rep = diff_and_record(
        vec![finding(
            "TOOLS.md",
            3,
            "bash is available",
            Verdict::Ok,
            "/bin/bash",
        )],
        StaleScanLedger::default(),
        day(0),
        1,
        "0.3.83",
    );
    assert!(rep.to_surface.is_empty());
    assert_eq!(rep.ledger.entries.len(), 1, "still recorded");
}

#[test]
fn ok_to_stale_change_resurfaces_and_stale_to_ok_clears() {
    let line = "use `legacy-helper` for uploads";
    // Cycle 1: healthy world.
    let c1 = diff_and_record(
        vec![finding(
            "SOUL.md",
            9,
            line,
            Verdict::Ok,
            "/usr/bin/legacy-helper",
        )],
        StaleScanLedger::default(),
        day(0),
        1,
        "0.3.83",
    );
    assert!(c1.to_surface.is_empty());

    // Cycle 2: the helper vanished — must RE-surface as a new flag.
    let c2 = diff_and_record(
        vec![finding(
            "SOUL.md",
            9,
            line,
            Verdict::Stale,
            "gone from PATH",
        )],
        c1.ledger,
        day(90_000),
        2,
        "0.3.83",
    );
    assert_eq!(c2.to_surface.len(), 1, "ok→stale resurfaces");

    // Cycle 3: user restored the binary — cleared, never silently dropped.
    let c3 = diff_and_record(
        vec![finding("SOUL.md", 9, line, Verdict::Ok, "restored")],
        c2.ledger,
        day(180_000),
        3,
        "0.3.83",
    );
    assert_eq!(c3.cleared.len(), 1, "stale→ok announces restoration");
    assert!(c3.to_surface.is_empty());
}

#[test]
fn flip_from_unverifiable_to_stale_surfaces_exactly_once() {
    let line = "point `service-endpoint` at prod";
    // Day 0: undecidable (e.g. binary mid-swap) — recorded silently.
    let u1 = diff_and_record(
        vec![finding(
            "MEMORY.md",
            4,
            line,
            Verdict::Unverifiable,
            "relative path",
        )],
        StaleScanLedger::default(),
        day(0),
        1,
        "0.3.83",
    );
    assert!(u1.to_surface.is_empty() && u1.cleared.is_empty());

    // Day 1: now decidable and DEAD — newly gained positive evidence is
    // news, so this MUST surface once even though a prior verdict existed.
    let u2 = diff_and_record(
        vec![finding(
            "MEMORY.md",
            4,
            line,
            Verdict::Stale,
            "now decidable: gone",
        )],
        u1.ledger,
        day(90_000),
        2,
        "0.3.83",
    );
    assert_eq!(u2.to_surface.len(), 1, "U->S is a genuine new flag");
    assert!(u2.cleared.is_empty());
    let stored = u2
        .ledger
        .entries
        .get(&ledger_key(&rule_hash(line.trim()), "anchor-4"))
        .expect("entry persisted");
    assert_eq!(stored.verdict, Verdict::Stale);

    // Day 2: same Stale verdict repeats — dedup suppresses the re-flag.
    let u3 = diff_and_record(
        vec![finding("MEMORY.md", 4, line, Verdict::Stale, "still gone")],
        u2.ledger,
        day(180_000),
        3,
        "0.3.83",
    );
    assert!(u3.to_surface.is_empty());
    assert_eq!(u3.suppressed, 1);
}

#[test]
fn stale_degrading_to_unverifiable_is_never_announced_as_resolved() {
    let line = "trust `legacy-sync` for migrations";
    // Day 0: genuinely stale, flagged as usual.
    let r1 = diff_and_record(
        vec![finding(
            "TOOLS.md",
            9,
            line,
            Verdict::Stale,
            "binary absent",
        )],
        StaleScanLedger::default(),
        day(0),
        1,
        "0.3.83",
    );
    assert_eq!(r1.to_surface.len(), 1);

    // Day 1: scan can no longer decide (transient fs failure). Absence of
    // evidence is NOT restoration: no cleared announcement, silent record.
    let r2 = diff_and_record(
        vec![finding(
            "TOOLS.md",
            9,
            line,
            Verdict::Unverifiable,
            "stat unavailable",
        )],
        r1.ledger,
        day(90_000),
        2,
        "0.3.83",
    );
    assert!(
        r2.to_surface.is_empty() && r2.cleared.is_empty(),
        "S->U must be silent"
    );
    let degraded = r2
        .ledger
        .entries
        .get(&ledger_key(&rule_hash(line.trim()), "anchor-9"))
        .expect("entry persisted");
    assert_eq!(degraded.verdict, Verdict::Unverifiable);

    // Day 2: positively re-verified OK — NOW resolution is real news.
    let r3 = diff_and_record(
        vec![finding(
            "TOOLS.md",
            9,
            line,
            Verdict::Ok,
            "restored upstream",
        )],
        r2.ledger,
        day(180_000),
        3,
        "0.3.83",
    );
    assert_eq!(r3.cleared.len(), 1, "S->Ok is a legitimate clear");
    assert!(r3.to_surface.is_empty());
}

#[test]
fn same_verdict_repeat_refreshes_cycle_stamp_and_evidence() {
    let line = "check `status-page` each morning";
    let r1 = diff_and_record(
        vec![finding("CODE.md", 21, line, Verdict::Ok, "v1 probe")],
        StaleScanLedger::default(),
        day(0),
        11,
        "0.3.83",
    );
    let r2 = diff_and_record(
        vec![finding("CODE.md", 21, line, Verdict::Ok, "v2 deeper probe")],
        r1.ledger,
        day(90_000),
        12,
        "0.3.83",
    );
    assert_eq!(r2.suppressed, 1);
    let key = ledger_key(&rule_hash(line.trim()), "anchor-21");
    // Identical (rule, anchor) pair re-verified NOW: cycle stamp rolls
    // forward and the fresher evidence wins.
    let refreshed = r2.ledger.entries.get(&key).expect("original entry lives");
    assert_eq!(refreshed.last_verified_cycle, 12);
    assert_eq!(refreshed.evidence, "v2 deeper probe");
}

// ------------------------------------------------- gated end-to-end runner

#[test]
fn gated_runner_full_lifecycle_dedup_then_force_run() {
    let dir = scratch_dir("e2e");
    let brain = dir.join("brain");
    std::fs::create_dir_all(&brain).unwrap();
    std::fs::write(
        brain.join("AGENTS.md"),
        "# rules\n\n- run `omdc-proxy` every night\n",
    )
    .unwrap();
    let cfg = Config::default();
    let ledger_path = dir.join("rsi/stale_scan.json");

    // Run 1 (cold): scans, finds the dead binary, persists the ledger.
    let r1 = run_scan_with_ledger(&cfg, &brain, &ledger_path, day(0), 1, "0.3.83");
    let rep1 = match r1 {
        crate::brain::rsi_stale_ledger::ScanRunOutcome::Ran { report, persisted } => {
            persisted.expect("ledger write succeeds");
            report
        }
        other => panic!("expected Ran, got {other:?}"),
    };
    assert_eq!(
        rep1.to_surface.len(),
        1,
        "cold run flags the stale binary once"
    );
    assert!(ledger_path.exists(), "ledger persisted to disk");

    // Run 2, seconds later, same binary: gate skips entirely.
    let r2 = run_scan_with_ledger(&cfg, &brain, &ledger_path, day(0) + 60, 2, "0.3.83");
    assert!(matches!(
        r2,
        crate::brain::rsi_stale_ledger::ScanRunOutcome::Skipped { .. }
    ));

    // Run 3, next day, SAME binary: scan runs, dedup suppresses the repeat.
    let r3 = run_scan_with_ledger(
        &cfg,
        &brain,
        &ledger_path,
        day(0) + SCAN_MIN_INTERVAL_SECS,
        3,
        "0.3.83",
    );
    let rep3 = match r3 {
        crate::brain::rsi_stale_ledger::ScanRunOutcome::Ran { report, persisted } => {
            persisted.expect("second ledger write succeeds");
            report
        }
        other => panic!("expected Ran, got {other:?}"),
    };
    assert!(
        rep3.to_surface.is_empty(),
        "identical re-scan flags nothing new"
    );
    assert!(rep3.suppressed >= 1, "repeat was suppressed by the ledger");

    // Run 4, immediately, NEW binary: force-run overrides the daily gate…
    let r4 = run_scan_with_ledger(
        &cfg,
        &brain,
        &ledger_path,
        day(0) + SCAN_MIN_INTERVAL_SECS + 60,
        4,
        "0.3.84",
    );
    assert!(matches!(
        r4,
        crate::brain::rsi_stale_ledger::ScanRunOutcome::Ran { .. }
    ));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn default_ledger_path_points_into_home_rsi_dir() {
    // Contract pin: the running app keeps machine state under ~/.opencrabs,
    // never inside any brain file or project dir.
    assert!(default_ledger_path().ends_with("rsi/stale_scan.json"));
}
