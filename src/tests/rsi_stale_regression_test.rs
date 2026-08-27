//! Task-6 regression suite (#1240): the seven end-to-end invariants of the
//! stale-claim scan, pinned against the REAL modules (scanner, ledger,
//! gated runner, cycle-prompt wiring, and the self_improve append-only
//! guard), not against mocks:
//!
//! 1. **zero-noise** — a healthy instance (live binaries, builtins, valid
//!    config keys, configured providers, real history lines) surfaces ZERO
//!    flags, changes NOTHING on disk, and builds the byte-identical
//!    pre-#1240 cycle prompt;
//! 2. **historical exemption** — dated incident mentions of dead tools
//!    never flag, proven on lines lifted from this install's own live
//!    brain files (violation ledgers, dated removal lessons, lesson
//!    headings), not on invented shapes;
//! 3. **prescription flagging** — a live imperative rule citing a binary
//!    that no longer exists IS flagged, with the reword-via-update action;
//! 4. **dedup second-run silence** — the same scan re-run reports zero new
//!    flags because the LEDGER remembers, proven independently of the
//!    cadence gate (a forced re-run past the gate still stays silent);
//! 5. **append-only** — a finding translates to `self_improve
//!    action='update'` input, never a deletion: the action vocabulary has
//!    no removal verb, the prompt block forbids agent edits on removal
//!    candidates, and the real `check_no_shrink` guard rejects a
//!    deletion-shaped file update while accepting the consolidation
//!    reword the finding recommends;
//! 6. **config-key source of truth** — verification hits the COMPILED
//!    schema witness, never `config.toml.example`: a key documented in
//!    the example but absent from the compiled types (`web_search.
//!    duckduckgo`) verifies Stale, while schema-only keys the example
//!    never mentions (`[doctor]`, `agent.eval_providers`) verify Ok;
//! 7. **ledger restart survival** — save → cold reload → next-day run
//!    still suppresses repeats and rolls the run stamp forward.
//!
//! Plus extraction **ordering safety** under deliberate fixture mutation:
//! inserting, reversing, and moving lines must shift line numbers exactly,
//! keep document order, attribute every anchor to the line that carries
//! it, and never let an exempted historical twin of a dead tool leak into
//! the findings.

use crate::brain::rsi::{StaleScanInput, build_cycle_prompt, stale_scan_prompt_block};
use crate::brain::rsi_stale_ledger::{
    ScanRunOutcome, StaleScanLedger, ledger_key, rule_hash, run_scan_with_ledger,
};
use crate::brain::rsi_stale_scan::{
    AnchorKind, FindingAction, LineClass, StaleFinding, Verdict, anchor_kind, classify_line,
    scan_brain_files,
};
use crate::brain::tools::brain_file_safety::{ShrinkCheck, check_no_shrink, is_rule_consolidation};
use crate::config::{Config, ProviderConfig};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

// ------------------------------------------------------------------ helpers

/// Unique scratch dir per call: parallel test files must never share a
/// tempdir (same hazard the scan/ledger/wire suites guard with atomics).
fn scratch_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "oc_stale_regress_{tag}_{}_{}",
        std::process::id(),
        n
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn day(secs: u64) -> u64 {
    1_700_000_000u64 + secs
}

/// Config with one provider actually configured, so provider anchors can
/// verify Ok in the healthy-instance fixture (same shape the scan suite
/// uses: `verify_provider` checks the configured-provider table).
fn cfg_with_anthropic() -> Config {
    let mut config = Config::default();
    config.providers.anthropic = Some(ProviderConfig {
        api_key: Some("sk-test".into()),
        ..Default::default()
    });
    config
}

/// The pre-#1240 prompt construction, replicated VERBATIM (same golden
/// baseline the wire suite pins). If the zero-noise path ever perturbs a
/// byte, this fails.
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

/// Unwrap a `Ran` outcome into its report, panicking loudly on `Skipped`.
fn ran(outcome: ScanRunOutcome) -> crate::brain::rsi_stale_ledger::ScanReport {
    match outcome {
        ScanRunOutcome::Ran { report, persisted } => {
            persisted.expect("ledger write succeeds in regression fixtures");
            report
        }
        other => panic!("expected Ran, got {other:?}"),
    }
}

fn run_gated(
    config: &Config,
    brain: &Path,
    ledger: &Path,
    t: u64,
    cycle: u64,
    version: &str,
) -> ScanRunOutcome {
    run_scan_with_ledger(config, brain, ledger, t, cycle, version)
}

/// The shared stale fixture: one dead binary prescription, one vanished
/// path prescription, and — the trap — a HISTORICAL twin of the same dead
/// binary carrying a command cue ("via") that would extract if the
/// exemption ever broke.
fn stale_brain(dir: &Path) {
    fs::write(
        dir.join("TOOLS.md"),
        concat!(
            "# tools\n",
            "\n",
            "- ALWAYS run `oc-release-wrap-9x` before sending release notes\n",
            "- check `/no/such/oc-regress-path.md` before deploy\n",
            "- Violation ledger: releases shipped via `oc-release-wrap-9x` before it was removed (2026-08-10)\n",
        ),
    )
    .expect("write TOOLS.md fixture");
}

const DEAD_BIN_LINE: &str = "- ALWAYS run `oc-release-wrap-9x` before sending release notes";
const DEAD_PATH_LINE: &str = "- check `/no/such/oc-regress-path.md` before deploy";

// ------------------------------------------------- 1. zero-noise invariant

/// A healthy instance — rules citing real live tooling, shell builtins,
/// valid config keys, a CONFIGURED provider, plus real dated history —
/// announces nothing, touches nothing, and builds the byte-identical
/// pre-#1240 cycle prompt.
#[test]
fn healthy_instance_yields_zero_flags_and_unchanged_behavior() {
    let dir = scratch_dir("zero-noise");
    let brain = dir.join("brain");
    fs::create_dir_all(&brain).unwrap();
    fs::write(
        brain.join("SOUL.md"),
        concat!(
            "# soul\n",
            "\n",
            "- always run `cargo clippy --all-features` before every commit\n",
            "- use `cd` when moving between directories\n",
        ),
    )
    .unwrap();
    fs::write(
        brain.join("TOOLS.md"),
        concat!(
            "# tools\n",
            "\n",
            "- set `[agent.approval_policy]` per channel before starting work\n",
            "- route vision work to provider `anthropic`\n",
        ),
    )
    .unwrap();
    // Real history line (violation-ledger tail on a live rule, AGENTS.md
    // shape) — exempt, and healthy by definition.
    fs::write(
        brain.join("AGENTS.md"),
        concat!(
            "# agents\n",
            "\n",
            "- **Always quote SSH commands**: Use double quotes for inner commands. Violations: 4, last: 2026-06-11\n",
        ),
    )
    .unwrap();
    let before: Vec<(String, String)> = ["SOUL.md", "TOOLS.md", "AGENTS.md"]
        .iter()
        .map(|f| (f.to_string(), fs::read_to_string(brain.join(f)).unwrap()))
        .collect();

    let config = cfg_with_anthropic();
    let ledger = dir.join("rsi/stale_scan.json");

    // Scanner level: findings may exist as Ok bookkeeping, but NOTHING is
    // stale — zero noise.
    let findings = scan_brain_files(&config, &brain);
    assert!(
        findings.iter().all(|f| f.verdict != Verdict::Stale),
        "healthy instance must yield zero stale verdicts: {findings:?}"
    );

    // Gated runner level: first run surfaces and clears nothing.
    let outcome = run_gated(&config, &brain, &ledger, day(0), 1, "0.3.83");
    let report = ran(outcome);
    assert!(
        report.to_surface.is_empty() && report.cleared.is_empty(),
        "zero-noise invariant: nothing to announce, got {:?}",
        report.to_surface
    );

    // Wire level: empty input → empty block → byte-identical baseline.
    let input = StaleScanInput::from_outcome(&ScanRunOutcome::Ran {
        report,
        persisted: Ok(()),
    });
    assert!(input.is_empty(), "healthy instance maps to empty input");
    let block = stale_scan_prompt_block(&input);
    assert!(block.is_empty());
    for opps in [
        vec![],
        vec!["tool bash failure rate 30% over 7d".to_string()],
    ] {
        assert_eq!(
            build_cycle_prompt(&opps, &block),
            pre_change_baseline(&opps),
            "healthy cycle must be byte-identical to the pre-#1240 prompt (shape {opps:?})"
        );
    }

    // Unchanged behavior, literally: the read-only scan must not have
    // touched a single fixture byte.
    for (name, content) in &before {
        assert_eq!(
            &fs::read_to_string(brain.join(name)).unwrap(),
            content,
            "scan must be read-only; {name} changed on disk"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------ 2. historical exemption (REAL lines)

/// Lines lifted from this install's LIVE brain files (~/.opencrabs), each
/// naming a dead thing — the exact pattern class the exemption exists
/// for. Sources noted per line; wording kept verbatim where possible.
#[test]
fn real_brain_history_patterns_stay_exempt() {
    let real_lines: &[&str] = &[
        // AGENTS.md: violation-ledger tail on a live rule.
        "- **Always quote SSH commands**: Use double quotes for inner commands. Violations: 4, last: 2026-06-11",
        // AGENTS.md: slash-command routing ledger with date + dead name.
        "**slash_command routing (hard rule):** Only invoke commands that exist in the Available Commands index. Do NOT guess or invent command names (e.g. /deploy, /release). Violations: 14 failures (37.8% rate), last: 2026-08-20 (\"Unknown provider 'custom'\").",
        // MEMORY.md: dated dead-tool swap lesson — mentions the retired
        // swap_supabase_postgres.py machinery in an imperative sentence.
        "**Supabase is gone from Flowise (Aug 2026):** the Supabase node was replaced by the Postgres node (swap executed 2026-08-09 via swap_supabase_postgres.py); never reference or reintroduce Supabase in flows.",
        // MEMORY.md: dated correction about a provider/model pair that
        // never existed — a dead provider name inside a dated lesson.
        "- \"zhipu/qwen-3.7-max-thinking\" is NOT a valid provider/model pair and was never valid. Do not suggest or use this pairing. User correction Jun 6 02:03 (session fd72101f).",
        // MEMORY.md: lesson-learned heading carrying a full date.
        "## Lesson Learned: Truelens Server Separation (June 8, 2026)",
        // MEMORY.md: retired homebrew tap, dated correction.
        "- OpenCrabs now installs via homebrew-core: brew install opencrabs. The separate homebrew-opencrabs tap is being retired; older docs referencing the tap install are wrong. Adolfo correction Aug 6 2026.",
    ];
    for line in real_lines {
        assert_eq!(
            classify_line(line),
            LineClass::HistoricalExempt,
            "real brain pattern must classify exempt: {line}"
        );
    }

    // And through the real scanner: the whole file of real history
    // produces zero findings of any verdict.
    let dir = scratch_dir("real-history");
    fs::write(dir.join("MEMORY.md"), real_lines.join("\n")).unwrap();
    let findings = scan_brain_files(&Config::default(), &dir);
    assert!(
        findings.is_empty(),
        "real history lines must yield zero findings, got {findings:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// -------------------------------------------- 3. prescription flagging

/// A live imperative rule (no dates, no ledger words) citing a binary
/// that exists nowhere on PATH IS flagged — once, with the exact
/// reword-via-update action and verifiable provenance.
#[test]
fn live_prescription_citing_dead_binary_is_flagged() {
    let dir = scratch_dir("prescription");
    let brain = dir.join("brain");
    fs::create_dir_all(&brain).unwrap();
    stale_brain(&brain);
    let ledger = dir.join("rsi/stale_scan.json");

    let outcome = run_gated(&Config::default(), &brain, &ledger, day(0), 1, "0.3.83");
    let report = ran(outcome);

    let bin: Vec<&StaleFinding> = report
        .to_surface
        .iter()
        .filter(|f| f.anchor == "oc-release-wrap-9x")
        .collect();
    assert_eq!(
        bin.len(),
        1,
        "dead binary must flag exactly once, got {:?}",
        report.to_surface
    );
    let f = bin[0];
    assert_eq!(f.verdict, Verdict::Stale);
    assert_eq!(f.action, FindingAction::RewordViaUpdate);
    assert_eq!(f.anchor_kind, AnchorKind::Binary);
    assert_eq!(f.file, "TOOLS.md");
    assert_eq!(f.line_no, 3);
    assert_eq!(f.line, DEAD_BIN_LINE);
    assert!(
        f.evidence.contains("PATH"),
        "evidence must be anchor-verified, got {}",
        f.evidence
    );

    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------- 4. dedup second-run silence

/// The second identical run is silent because the LEDGER remembers the
/// flag — not merely because the cadence gate skipped the scan. Proven by
/// forcing a real re-run past the gate (binary version bump): the scan
/// executes, re-verifies the same dead anchors, and still surfaces
/// nothing new.
#[test]
fn dedup_second_run_silence_is_ledger_not_gate() {
    let dir = scratch_dir("dedup");
    let brain = dir.join("brain");
    fs::create_dir_all(&brain).unwrap();
    stale_brain(&brain);
    let config = Config::default();
    let ledger = dir.join("rsi/stale_scan.json");

    // Run 1: both stale anchors surface (dead binary + vanished path);
    // the historical twin of the same binary contributes nothing.
    let report1 = ran(run_gated(&config, &brain, &ledger, day(0), 1, "0.3.83"));
    assert_eq!(report1.to_surface.len(), 2, "{:?}", report1.to_surface);

    // Run 2, one minute later, same binary: the cadence gate skips — this
    // is gate silence, NOT yet proof of dedup.
    let outcome2 = run_gated(&config, &brain, &ledger, day(0) + 60, 2, "0.3.83");
    assert!(
        matches!(outcome2, ScanRunOutcome::Skipped { .. }),
        "within-window re-run must be gated, got {outcome2:?}"
    );

    // Run 3, two minutes in, NEW binary version: gate force-opens, the
    // scan REALLY runs — and the ledger suppresses both repeats.
    let report3 = ran(run_gated(
        &config,
        &brain,
        &ledger,
        day(0) + 120,
        3,
        "0.3.84",
    ));
    assert!(
        report3.to_surface.is_empty(),
        "forced re-run must stay silent on identical findings: {:?}",
        report3.to_surface
    );
    assert_eq!(
        report3.suppressed, 2,
        "both repeats must be counted as ledger-suppressed"
    );
    assert!(report3.cleared.is_empty());

    let _ = fs::remove_dir_all(&dir);
}

// --------------------------------------------------- 5. append-only

/// A finding translates to `self_improve action='update'` input, never a
/// deletion — pinned at all three levels that could regress:
/// the action vocabulary (no removal verb exists), the prompt block
/// (reword set says update + never-deletes; removal candidates say
/// do-NOT-edit), and the REAL executor guard (`check_no_shrink`), driven
/// with the two file updates the finding's actions imply.
#[test]
fn append_only_finding_yields_update_input_never_deletion() {
    let dir = scratch_dir("append-only");
    let brain = dir.join("brain");
    fs::create_dir_all(&brain).unwrap();
    stale_brain(&brain);
    let ledger = dir.join("rsi/stale_scan.json");

    let report = ran(run_gated(
        &Config::default(),
        &brain,
        &ledger,
        day(0),
        1,
        "0.3.83",
    ));
    let input = StaleScanInput::from_outcome(&ScanRunOutcome::Ran {
        report,
        persisted: Ok(()),
    });

    // Partition: dead binary → agent may reword via update; vanished path
    // → owner sign-off, never agent-edited.
    assert_eq!(input.reword_via_update.len(), 1, "{input:?}");
    assert_eq!(
        input.reword_via_update[0].action,
        FindingAction::RewordViaUpdate
    );
    assert_eq!(input.owner_signoff.len(), 1, "{input:?}");
    assert_eq!(input.owner_signoff[0].action, FindingAction::SurfaceToUser);

    // Vocabulary sweep: no finding action is a removal, on the wire or in
    // the slug accessor.
    for f in input
        .reword_via_update
        .iter()
        .chain(input.owner_signoff.iter())
    {
        let slug = serde_json::to_string(&f.action).unwrap();
        let accessor = f.action.as_str();
        for s in [slug.as_str(), accessor] {
            assert!(
                !s.contains("delete") && !s.contains("remove") && !s.contains("prune"),
                "action vocabulary must never name a removal: {s}"
            );
        }
    }

    // Prompt bytes: the reword recommendation is the update verb with the
    // never-deletes guarantee; removal candidates are explicitly fenced.
    let block = stale_scan_prompt_block(&input);
    assert!(
        block.contains("self_improve action='update'"),
        "reword set must recommend update, got:\n{block}"
    );
    assert!(
        block.to_lowercase().contains("never deletes"),
        "prompt must state the never-deletes guarantee, got:\n{block}"
    );
    assert!(
        block.contains("do NOT edit these yourself"),
        "owner-signoff fence missing, got:\n{block}"
    );

    // Executor level: drive the REAL append-only guard with the two file
    // updates the finding implies. Fixture mirrors the AGENTS.md bold-rule
    // shape (directive at line start — the identifier form the
    // consolidation path requires, #858).
    let guard_dir = scratch_dir("append-only-guard");
    let guard_path = guard_dir.join("TOOLS.md");
    let bold_rule =
        "**Release wrap guard**: ALWAYS run `oc-release-wrap-9x` before sending release notes";
    let existing = format!("# tools\n\n{bold_rule}\n{DEAD_PATH_LINE}\n");
    fs::write(&guard_path, &existing).unwrap();

    // (a) Deletion-shaped update: drop the whole rule line. The guard
    // must reject it — autonomously reachable code paths cannot remove.
    let deletion = existing.replacen(&format!("{bold_rule}\n"), "", 1);
    assert_ne!(deletion, existing, "mutation must actually shrink");
    match check_no_shrink(
        &guard_path,
        &existing,
        &deletion,
        false, // dedup_intent
        false, // cleanup_intent — never available to autonomous RSI
        is_rule_consolidation(bold_rule, ""),
    ) {
        ShrinkCheck::Rejected { message } => assert!(
            message.contains("append-only"),
            "rejection must name the append-only policy: {message}"
        ),
        ShrinkCheck::Allowed => panic!("deletion-shaped update must be rejected"),
    }

    // (b) Reword-shaped update — exactly what the finding's action
    // recommends: same rule, same directive, dead anchor replaced by a
    // live one. The guard allows it as a bounded consolidation.
    let reworded = "**Release wrap guard**: ALWAYS run `cargo build` before sending release notes";
    let updated = existing.replacen(bold_rule, reworded, 1);
    assert_eq!(
        check_no_shrink(
            &guard_path,
            &existing,
            &updated,
            false,
            false,
            is_rule_consolidation(bold_rule, reworded),
        ),
        ShrinkCheck::Allowed,
        "the reword the finding recommends must pass the real guard"
    );

    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&guard_dir);
}

// --------------------------------- 6. config keys: schema, not the example

/// `verify_config_key` must consult the COMPILED schema witness, never
/// `config.toml.example`. Proven with the example file itself as the
/// control: it documents a key the compiled types dropped
/// (`web_search.duckduckgo` — `WebSearchProviders` has only exa/brave),
/// and it omits keys the compiled schema has (`[doctor]`,
/// `agent.eval_providers`, `agent.redact_group`). If the verifier read
/// the example, duckduckgo would verify Ok and the schema-only keys would
/// fail — the exact opposite of what it returns.
#[test]
fn config_key_verification_targets_embedded_schema_not_example() {
    // Control: assert the drift preconditions against the actual example
    // file on disk, so a future sync that erases the drift case fails
    // HERE with an explanation instead of silently weakening the test.
    let example_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config.toml.example");
    let example = fs::read_to_string(&example_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", example_path.display()));
    assert!(
        example.contains("[providers.web_search.duckduckgo]"),
        "precondition drifted: example no longer documents web_search.duckduckgo — \
         pick a fresh example-only key for this test"
    );
    for schema_only in ["[doctor]", "eval_providers", "redact_group"] {
        assert!(
            !example.contains(schema_only),
            "precondition drifted: example now documents {schema_only} — \
             pick a fresh schema-only key for this test"
        );
    }

    use crate::brain::rsi_stale_scan::verify_config_key;
    // Documented in the example, ABSENT from the compiled types: the
    // schema wins → Stale. (An example-driven verifier would say Ok.)
    assert_eq!(
        verify_config_key("[providers.web_search.duckduckgo.api_key]"),
        Verdict::Stale,
        "example-documented but schema-absent key must verify stale"
    );
    // In the compiled schema, NEVER mentioned in the example: still Ok.
    // (An example-driven verifier would fail these.)
    assert_eq!(verify_config_key("[doctor]"), Verdict::Ok);
    assert_eq!(verify_config_key("[agent.eval_providers]"), Verdict::Ok);
    assert_eq!(verify_config_key("[agent.redact_group]"), Verdict::Ok);

    // And through the scanner: a prescription citing the example-only key
    // is flagged with the reword action, while a prescription citing a
    // schema-only key is not.
    let dir = scratch_dir("cfgkeys");
    fs::write(
        dir.join("CODE.md"),
        concat!(
            "# code\n",
            "\n",
            "- set `[providers.web_search.duckduckgo.api_key]` for search fallback\n",
            "- set `[agent.eval_providers]` before running evals\n",
        ),
    )
    .unwrap();
    let findings = scan_brain_files(&Config::default(), &dir);
    let stale: Vec<&StaleFinding> = findings
        .iter()
        .filter(|f| f.verdict == Verdict::Stale)
        .collect();
    assert_eq!(stale.len(), 1, "{findings:?}");
    assert_eq!(stale[0].anchor, "[providers.web_search.duckduckgo.api_key]");
    assert_eq!(stale[0].action, FindingAction::RewordViaUpdate);
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------ 7. ledger restart survival

/// The ledger is the memory that makes dedup work, so it must survive a
/// restart: after save → cold `load` (new handle, disk bytes), the
/// run stamp, binary version, and every (rule, anchor) entry are intact,
/// and the next-day run on the reloaded ledger stays silent.
#[test]
fn ledger_survives_restart_and_keeps_second_run_silent() {
    let dir = scratch_dir("restart");
    let brain = dir.join("brain");
    fs::create_dir_all(&brain).unwrap();
    stale_brain(&brain);
    let config = Config::default();
    let ledger_path = dir.join("rsi/stale_scan.json");
    let t1 = day(0);
    let t2 = day(0) + 25 * 3600; // past the daily gate

    // Run 1: both stale anchors surface once and are persisted.
    let report1 = ran(run_gated(&config, &brain, &ledger_path, t1, 1, "0.3.83"));
    assert_eq!(report1.to_surface.len(), 2);

    // Simulated restart: brand-new handle, cold read from disk bytes.
    let revived = StaleScanLedger::load(&ledger_path)
        .expect("ledger must be readable after the run that wrote it");
    assert_eq!(revived.binary_version, "0.3.83");
    assert_eq!(revived.last_run_unix, t1, "gate reference must survive");
    assert_eq!(
        revived.entries.len(),
        2,
        "both anchors recorded: {revived:?}"
    );
    for (line, anchor) in [
        (DEAD_BIN_LINE, "oc-release-wrap-9x"),
        (DEAD_PATH_LINE, "/no/such/oc-regress-path.md"),
    ] {
        let key = ledger_key(&rule_hash(line), anchor);
        let entry = revived
            .entries
            .get(&key)
            .unwrap_or_else(|| panic!("entry {key} must survive restart"));
        assert_eq!(entry.verdict, Verdict::Stale);
        assert!(entry.outstanding_stale, "announced flag stays outstanding");
    }

    // Run 2 next day (runner cold-loads the ledger again): dedup across
    // the restart boundary — silent, with the stamp rolled forward.
    let report2 = ran(run_gated(&config, &brain, &ledger_path, t2, 2, "0.3.83"));
    assert!(
        report2.to_surface.is_empty(),
        "second run after restart must stay silent: {:?}",
        report2.to_surface
    );
    assert_eq!(report2.suppressed, 2);
    let revived2 = StaleScanLedger::load(&ledger_path).unwrap();
    assert_eq!(revived2.last_run_unix, t2, "run stamp rolls forward");

    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------- extraction ordering safety (mutations)

/// Summary tuple for order-sensitive comparisons: verdicts and kinds are
/// compared; line numbers are asserted separately where expected.
fn tuples(findings: &[StaleFinding]) -> Vec<(usize, String, AnchorKind, Verdict)> {
    findings
        .iter()
        .map(|f| (f.line_no, f.anchor.clone(), f.anchor_kind, f.verdict))
        .collect()
}

/// Real-pattern base fixture. The ONLY stale line is the plain
/// prescription naming `cmd-wrap`; its dated removal lesson and its
/// violation-ledger twin (same dead tool!) are history. Position 3 is a
/// live prescription with no anchors at all.
fn base_fixture() -> Vec<String> {
    vec![
        "# MEMORY.md".to_string(),
        String::new(),
        "- NEVER push to main without explicit approval".to_string(),
        "- Violations: 3, last 2026-08-22 (session fd72101f)".to_string(),
        "- always run `cmd-wrap` before replying about releases".to_string(),
        "- `cmd-wrap` removed Aug 10 — use plain bash now".to_string(),
        "- violation ledger: banned `cmd-wrap` usage (removed 2026-08-10, session f00d)"
            .to_string(),
    ]
}

fn write_and_scan(lines: &[String]) -> Vec<StaleFinding> {
    let dir = scratch_dir("mutate");
    fs::write(dir.join("MEMORY.md"), lines.join("\n")).expect("write fixture");
    let findings = scan_brain_files(&Config::default(), &dir);
    let _ = fs::remove_dir_all(&dir);
    findings
}

fn expect_single_cmdwrap_flag(findings: Vec<StaleFinding>, label: &str) {
    let summary: Vec<&str> = findings.iter().map(|f| f.line.as_str()).collect();
    assert_eq!(
        findings.len(),
        1,
        "{label}: expected exactly one flag, got {summary:?}"
    );
    assert_eq!(findings[0].anchor, "cmd-wrap", "{label}: wrong anchor");
    assert!(
        findings[0].line.contains("always run"),
        "{label}: flagged the historical mention instead of the imperative"
    );
}

/// Deliberate mutation #1 — full reversal: exemption and flagging are
/// position-independent; count, anchor, and flagged line identical.
#[test]
fn reversed_fixture_flags_the_same_single_line() {
    expect_single_cmdwrap_flag(write_and_scan(&base_fixture()), "base");
    let mut reversed = base_fixture();
    reversed.reverse();
    expect_single_cmdwrap_flag(write_and_scan(&reversed), "reversed");
}

/// Deliberate mutation #2 — the flagged imperative moved ABOVE every
/// exemption, forcing the scanner past it before it meets the historical
/// twins of the same tool. Still exactly one flag, naming the imperative.
#[test]
fn prescription_moved_above_history_still_exactly_one_flag() {
    let mut order = base_fixture();
    let imperative = order.remove(4);
    order.insert(2, imperative);
    expect_single_cmdwrap_flag(write_and_scan(&order), "imperative-first");
}

/// Deliberate mutation #3 — insertions above and an append below: line
/// numbers shift by exactly the inserted count, findings stay in document
/// order, every anchor is attributed to the line that carries it, the
/// inserted historical twin of the dead tool contributes nothing, and the
/// scan is deterministic on identical bytes.
#[test]
fn mutated_fixture_extraction_orders_and_attributes_correctly() {
    let dir = scratch_dir("mutate-insert");
    let path = dir.join("TOOLS.md");

    let v1 = concat!(
        "# tools\n",
        "\n",
        "- run `oc-regress-dead-a` nightly\n",
        "- check `/no/such/oc-regress-path.md` before deploy\n",
        "- always run `cargo clippy --all-features` before commit\n",
    );
    fs::write(&path, v1).unwrap();
    let f1 = scan_brain_files(&Config::default(), &dir);
    // Document order: dead binary (stale), dead path (stale), live cargo
    // command (Ok bookkeeping).
    assert_eq!(
        tuples(&f1),
        vec![
            (
                3,
                "oc-regress-dead-a".to_string(),
                AnchorKind::Binary,
                Verdict::Stale
            ),
            (
                4,
                "/no/such/oc-regress-path.md".to_string(),
                AnchorKind::FilePath,
                Verdict::Stale
            ),
            (
                5,
                "cargo clippy --all-features".to_string(),
                AnchorKind::Binary,
                Verdict::Ok
            ),
        ],
        "base scan: {f1:?}"
    );

    // THE MUTATION: two lines inserted above the prescriptions (one prose,
    // one a historical twin of dead-a carrying a command cue — the exact
    // line that would leak if classification order regressed) and one new
    // dead-binary prescription appended below.
    let v2 = concat!(
        "# tools\n",
        "\n",
        "some prose about nothing in particular\n",
        "- Violation ledger: releases shipped via `oc-regress-dead-a` before it was removed (2026-08-10)\n",
        "- run `oc-regress-dead-a` nightly\n",
        "- check `/no/such/oc-regress-path.md` before deploy\n",
        "- always run `cargo clippy --all-features` before commit\n",
        "- run `oc-regress-dead-b` nightly\n",
    );
    fs::write(&path, v2).unwrap();
    let f2 = scan_brain_files(&Config::default(), &dir);

    // Every pre-existing finding shifted by exactly the two inserted
    // lines; the appended prescription flags at its own new line; the
    // inserted historical twin (line 4) contributes NOTHING.
    assert_eq!(
        tuples(&f2),
        vec![
            (
                5,
                "oc-regress-dead-a".to_string(),
                AnchorKind::Binary,
                Verdict::Stale
            ),
            (
                6,
                "/no/such/oc-regress-path.md".to_string(),
                AnchorKind::FilePath,
                Verdict::Stale
            ),
            (
                7,
                "cargo clippy --all-features".to_string(),
                AnchorKind::Binary,
                Verdict::Ok
            ),
            (
                8,
                "oc-regress-dead-b".to_string(),
                AnchorKind::Binary,
                Verdict::Stale
            ),
        ],
        "mutated scan: {f2:?}"
    );
    assert!(
        !f2.iter().any(|f| f.line_no == 4),
        "inserted historical twin must not leak into findings"
    );

    // Ordering: findings are strictly in document order.
    let line_nos: Vec<usize> = f2.iter().map(|f| f.line_no).collect();
    let mut sorted = line_nos.clone();
    sorted.sort_unstable();
    assert_eq!(line_nos, sorted, "findings must follow document order");

    // Attribution: every anchor really sits on the line it is attributed
    // to (off-by-one line numbers are the classic extraction bug).
    let lines: Vec<&str> = v2.lines().collect();
    for f in &f2 {
        assert!(
            lines[f.line_no - 1].contains(&f.anchor),
            "anchor {} misattributed to line {} ({:?})",
            f.anchor,
            f.line_no,
            lines[f.line_no - 1]
        );
    }

    // Determinism on identical bytes: a second scan is bit-identical.
    let f2b = scan_brain_files(&Config::default(), &dir);
    assert_eq!(tuples(&f2), tuples(&f2b), "scan must be deterministic");

    let _ = fs::remove_dir_all(&dir);
}

/// Cue-vocabulary pin (#1240 review): "invoke" and "call" count as command
/// cues so prescriptions phrased this way actually verify. Regression
/// witness — the mutation fixtures above silently produced ZERO findings
/// before these two words joined `command_like`'s cue list.
#[test]
fn invoke_and_call_are_command_cues() {
    assert_eq!(
        anchor_kind(
            "cmd-wrap",
            "- always invoke `cmd-wrap` before replying about releases"
        ),
        Some(AnchorKind::Binary),
        "invoke must be a command cue"
    );
    assert_eq!(
        anchor_kind(
            "opencrabs",
            "- call `opencrabs --version` after each evolve"
        ),
        Some(AnchorKind::Binary),
        "call must be a command cue"
    );
}

/// Past-tense guard: "was invoked" / "is called by" carry no trailing-space
/// cue and must stay unclassified — narration is not a prescription.
#[test]
fn past_tense_narration_stays_unclassified() {
    assert_eq!(
        anchor_kind("cmd-wrap", "- the tool invoked was `cmd-wrap` that day"),
        None,
        "'invoked' (past tense) must not become a Binary anchor"
    );
}
