//! Persistent ledger + dedup + cadence gate for the stale-claim scan
//! (#1240, RFC step 6 — the memory half of the scan).
//!
//! [`super::rsi_stale_scan`] classifies, extracts, and verifies — it is
//! stateless and forgets everything between runs. This module is the
//! counterweight that remembers:
//!
//! - **Ledger** — `rsi/stale_scan.json`, keyed by `(rule_hash, anchor)`
//!   (hash of the rule TEXT, not its line number: a rule that moves keeps
//!   its history). Each entry records `verdict`, `evidence`, and
//!   `last_verified_cycle`.
//! - **Dedup** — same-verdict repeats are SUPPRESSED: a stale rule already
//!   flagged is not re-flagged every cycle, so the second identical scan
//!   run reports zero new flags.
//! - **Re-surface on change** — a verdict CHANGE never passes silently:
//!   ok → stale surfaces as a new flag; stale → ok (e.g. the binary was
//!   restored) surfaces as *cleared*. Findings in either direction are
//!   information for the cycle agent, not noise.
//! - **Cadence gate** — the scan runs at most once per day, checked
//!   against the ledger's `last_run_unix`, PLUS a force-run trigger: a
//!   binary version change since the last ledger write re-opens the gate
//!   immediately, because a fresh binary may have changed the world the
//!   rules describe (new provider registry, renamed config keys).
//!
//! The ledger is machine state, not a brain file: it is rewritten freely
//! (atomically — write `.tmp`, rename), and an unreadable or foreign-schema
//! ledger degrades to first-run semantics rather than an error, so a
//! corrupt file can never wedge the scan.
//!
//! Tests live in `src/tests/rsi_stale_ledger_test.rs` (house rule: no
//! inline test modules).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::rsi_stale_scan::{StaleFinding, Verdict, scan_brain_files};

/// Minimum wall-clock gap between two stale scans (#1240 task 4: once per
/// day, checked against the ledger's last-run timestamp).
pub const SCAN_MIN_INTERVAL_SECS: u64 = 24 * 3600;

/// Ledger file format version. A ledger on disk with any other version is
/// ignored (first-run semantics) rather than misread.
pub const LEDGER_SCHEMA_VERSION: u32 = 1;

/// Where the running app keeps its ledger: `~/.opencrabs/rsi/stale_scan.json`.
pub fn default_ledger_path() -> PathBuf {
    crate::config::opencrabs_home().join("rsi/stale_scan.json")
}

/// One ledger record: what the world said about one (rule, anchor) pair the
/// last time the scan verified it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LedgerEntry {
    pub verdict: Verdict,
    pub evidence: String,
    pub last_verified_cycle: u64,
    /// True while an *announced* Stale finding awaits resolution by
    /// positive re-verification (`Ok`). Survives silent `Unverifiable`
    /// degradations so a later Ok still resolves what the user was told;
    /// never set when no flag was ever announced.
    #[serde(default)]
    pub outstanding_stale: bool,
}

/// Persistent dedup state for the stale scan (`rsi/stale_scan.json`).
/// `entries` is keyed by `(rule_hash, anchor)` — see [`ledger_key`]. A
/// `BTreeMap` on purpose: deterministic serialization keeps the ledger
/// diff-stable across writes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StaleScanLedger {
    pub schema_version: u32,
    /// Binary version in effect when the ledger was last written; a change
    /// force-runs the next scan regardless of the daily gate.
    pub binary_version: String,
    /// Unix timestamp of the last completed scan run (gate reference).
    pub last_run_unix: u64,
    pub entries: BTreeMap<String, LedgerEntry>,
}

impl Default for StaleScanLedger {
    fn default() -> Self {
        Self {
            schema_version: LEDGER_SCHEMA_VERSION,
            binary_version: String::new(),
            last_run_unix: 0,
            entries: BTreeMap::new(),
        }
    }
}

impl StaleScanLedger {
    /// Load the ledger from disk. `None` (→ first-run semantics: run the
    /// scan, rebuild the ledger) when the file is missing, corrupt JSON, or
    /// written under a different schema version. The ledger is machine
    /// state, so "unreadable" degrades to "start over", never to an error.
    pub fn load(path: &Path) -> Option<Self> {
        let raw = std::fs::read_to_string(path).ok()?;
        let ledger: StaleScanLedger = serde_json::from_str(&raw).ok()?;
        (ledger.schema_version == LEDGER_SCHEMA_VERSION).then_some(ledger)
    }

    /// Persist the ledger atomically (write `.tmp`, rename over the target)
    /// so a crash mid-write can never leave a half-written ledger behind.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }
}

/// Stable content hash of one rule line (FNV-1a 64, 16 hex chars).
/// Computed over the TRIMMED line so whitespace-only edits do not fork a
/// rule's ledger history, and over the TEXT rather than the line number so
/// a rule that moves within (or between) brain files keeps its dedup state.
pub fn rule_hash(line: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in line.trim().as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Ledger key for one (rule, anchor) pair. The hash prefix is fixed-width
/// (16 hex chars) so the key never needs to be parsed back apart.
pub fn ledger_key(rule_hash: &str, anchor: &str) -> String {
    format!("{rule_hash}:{anchor}")
}

/// Decision of the cadence gate (#1240 task 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CadenceDecision {
    /// Run the scan now. `forced` is true only for the binary-version
    /// trigger — a run that ignores the daily gate because the binary
    /// itself changed since the last ledger write.
    Run { forced: bool, reason: &'static str },
    /// Not now: last run was within [`SCAN_MIN_INTERVAL_SECS`] and the
    /// binary version is unchanged — a re-run would only re-derive what
    /// the ledger already suppresses.
    Skip { reason: &'static str },
}

/// Once-per-day gate plus the binary-version force trigger. Pure: the clock
/// (`now_unix`) and version are inputs, so every branch is unit-testable.
pub fn cadence_gate(
    ledger: Option<&StaleScanLedger>,
    now_unix: u64,
    binary_version: &str,
) -> CadenceDecision {
    let Some(ledger) = ledger else {
        return CadenceDecision::Run {
            forced: false,
            reason: "first run: no ledger on disk",
        };
    };
    if ledger.binary_version != binary_version {
        return CadenceDecision::Run {
            forced: true,
            reason: "binary version changed since last ledger write",
        };
    }
    if now_unix.saturating_sub(ledger.last_run_unix) >= SCAN_MIN_INTERVAL_SECS {
        return CadenceDecision::Run {
            forced: false,
            reason: "daily cadence elapsed",
        };
    }
    CadenceDecision::Skip {
        reason: "scan already ran within 24h on this binary version",
    }
}

/// Result of diffing one scan's findings against the ledger.
#[derive(Debug)]
pub struct ScanReport {
    /// NEW flags to surface: first-time stale findings, plus previously-ok
    /// or previously-unknown anchors whose verdict CHANGED to stale.
    pub to_surface: Vec<StaleFinding>,
    /// Previously-STALE anchors whose verdict changed to Ok (e.g. the
    /// binary was restored). Re-surfaced as cleared — a stale rule going
    /// quiet is information the cycle agent should see, never a silent
    /// drop.
    pub cleared: Vec<StaleFinding>,
    /// Same-verdict repeats that were deliberately NOT re-flagged.
    pub suppressed: usize,
    /// The updated ledger, ready to `save`.
    pub ledger: StaleScanLedger,
}

/// Diff findings against the ledger and fold them in (verdict, evidence,
/// `last_verified_cycle`, run timestamp, binary version). Pure with respect
/// to the outside world: disk I/O is the caller's job, which is exactly
/// what makes the dedup semantics unit-testable.
///
/// Asymmetric flip semantics (the property this module guarantees):
///
/// - Flips INTO [`Verdict::Stale`] always surface, including from
///   [`Verdict::Unverifiable`]: newly gained positive evidence is news,
///   not a repeat.
/// - Flips OUT of [`Verdict::Stale`] surface as `cleared` only when the
///   anchor was *positively re-verified* (e.g. now `Ok`). Degrading to
///   [`Verdict::Unverifiable`] records silently — absence of evidence is
///   not restoration, so nothing is announced as resolved.
pub fn diff_and_record(
    findings: Vec<StaleFinding>,
    mut ledger: StaleScanLedger,
    now_unix: u64,
    cycle: u64,
    binary_version: &str,
) -> ScanReport {
    let mut to_surface = Vec::new();
    let mut cleared = Vec::new();
    let mut suppressed = 0usize;

    for finding in findings {
        let key = ledger_key(&rule_hash(&finding.line), &finding.anchor);
        match ledger.entries.get(&key).cloned() {
            // First sighting: record; flag only if actually stale (an Ok
            // first sighting is bookkeeping, not news).
            None => {
                if finding.verdict == Verdict::Stale {
                    to_surface.push(finding.clone());
                }
                ledger.entries.insert(
                    key,
                    LedgerEntry {
                        verdict: finding.verdict,
                        evidence: finding.evidence.clone(),
                        last_verified_cycle: cycle,
                        outstanding_stale: finding.verdict == Verdict::Stale,
                    },
                );
            }
            // Verdict changed: re-surface in the direction of the change.
            Some(prev) if prev.verdict != finding.verdict => {
                if finding.verdict == Verdict::Stale {
                    to_surface.push(finding.clone());
                } else if prev.outstanding_stale && finding.verdict == Verdict::Ok {
                    // Positive re-verification of an announced flag:
                    // resolution is real news even after an intervening
                    // silent Unverifiable degradation.
                    cleared.push(finding.clone());
                }
                ledger.entries.insert(
                    key,
                    LedgerEntry {
                        verdict: finding.verdict,
                        evidence: finding.evidence.clone(),
                        last_verified_cycle: cycle,
                        outstanding_stale: match finding.verdict {
                            Verdict::Stale => true,
                            Verdict::Ok => false,
                            Verdict::Unverifiable => prev.outstanding_stale,
                        },
                    },
                );
            }
            // Same verdict as last time: skip re-flagging, but the anchor
            // was re-verified NOW — refresh the cycle stamp and evidence.
            Some(_) => {
                suppressed += 1;
                if let Some(entry) = ledger.entries.get_mut(&key) {
                    entry.last_verified_cycle = cycle;
                    entry.evidence = finding.evidence.clone();
                }
            }
        }
    }

    ledger.binary_version = binary_version.to_string();
    ledger.last_run_unix = now_unix;
    ScanReport {
        to_surface,
        cleared,
        suppressed,
        ledger,
    }
}

/// Outcome of one gated scan run.
#[derive(Debug)]
pub enum ScanRunOutcome {
    /// The cadence gate said not now. Zero flags reported by definition.
    Skipped { reason: &'static str },
    /// The scan ran. `persisted` is `Err` when the ledger write failed —
    /// findings are still returned; the cost is only that the next
    /// successful run will re-flag (dedup history was not saved).
    Ran {
        report: ScanReport,
        persisted: Result<(), String>,
    },
}

/// Full gated scan pass: load ledger → cadence gate → scan brain files →
/// diff/dedup against ledger → persist. This is the entry point the RSI
/// cycle calls (once per cycle; the gate makes the actual scan daily, or
/// immediate on binary version change). `now_unix`, `cycle`, and
/// `binary_version` are inputs so tests never depend on wall-clock, a real
/// `rsi/cycle_number`, or the compiled-in crate version.
pub fn run_scan_with_ledger(
    config: &crate::config::Config,
    brain_root: &Path,
    ledger_path: &Path,
    now_unix: u64,
    cycle: u64,
    binary_version: &str,
) -> ScanRunOutcome {
    let ledger = StaleScanLedger::load(ledger_path).unwrap_or_default();
    match cadence_gate(Some(&ledger), now_unix, binary_version) {
        CadenceDecision::Skip { reason } => ScanRunOutcome::Skipped { reason },
        CadenceDecision::Run { .. } => {
            let findings = scan_brain_files(config, brain_root);
            let report = diff_and_record(findings, ledger, now_unix, cycle, binary_version);
            let persisted = report.ledger.save(ledger_path).map_err(|e| e.to_string());
            ScanRunOutcome::Ran { report, persisted }
        }
    }
}
