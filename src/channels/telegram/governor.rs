//! Proactive per-peer flood governors for Telegram forum chats (#1211).
//!
//! Telegram-side FLOOD_WAIT 429s grow daily on multi-session forum
//! deployments (391 chat-action 429s on Aug 25 alone, worst second 36 failed
//! typing attempts): a forum supergroup is ONE peer (`chat_id`) regardless of
//! topic, the typing indicator re-fires every 4 s per active turn, and the
//! flow loop re-renders chrome every 1.5 s tick. The existing handling is
//! reactive only ([`super::rate_limit`] — parse `Retry after`, sleep): nothing
//! prevents hitting the limit, and retries consume more of the same bucket,
//! amplifying into storms.
//!
//! This module enforces three independent governors keyed by `chat_id`,
//! layered BESIDE the reactive backstop, which stays as the last line of
//! defence:
//!
//! - **G1 typing** ([`admit_chat_action`]): token bucket ~1 call / 3 s per
//!   forum peer, burst capacity 8. Concurrent sessions in one topic collapse
//!   into that single refresh because `sendChatAction` is thread-scoped and
//!   stateless. Under pressure a caller holds briefly for refill instead of
//!   hammering (hold-and-release); a refresh held past
//!   [`Limits::typing_max_hold`] is dropped — the indicator is cosmetic and
//!   the next tick re-fires it.
//! - **G2 edits** ([`edit_admission`] + [`EditClass`]): token bucket
//!   ~30/min per forum peer. On an empty bucket the priority drop ladder
//!   applies — clock → brain preview → intermediary flow updates → status
//!   line — because each dropped class self-heals: every flow refresh
//!   re-renders FULL current state, so the next admitted edit carries the
//!   dropped one's content. **Finals are never dropped**: settle renders and
//!   plan-card refreshes queue latest-wins per message id ([`PendingFinal`])
//!   and a drainer flushes them as tokens refill.
//! - **G3 sends** ([`pace_send`]): paced ~1/s per chat under an ~18/min group
//!   ceiling with a small burst absorb. Sends delay rather than drop (#297),
//!   so pacing FAILS OPEN after [`SEND_MAX_HOLD`] instead of ever discarding.
//!
//! Scope is forums-only initially. A negative chat id (groups) becomes a
//! governed peer once any call is observed carrying a topic id — positive ids
//! are DMs and are never touched, so DM-only surfaces (draft streaming et al.)
//! pass through unchanged. Direct user-command replies stay outside G3 by
//! design (≤1 message per human action); the reactive backstop covers them.
//!
//! Telemetry: per-forum counters for admissions, ladder drops per class and
//! throttle milliseconds, summarized by one periodic INFO line
//! ([`summary_loop`]).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use teloxide::prelude::Requester;
use teloxide::types::{ChatId, MessageId, ParseMode};
use teloxide::Bot;

use crate::config::Config;

/// Virtual-clock offset used ONLY by `cfg(test)` builds (`gate_now`). Tests
/// advance it by hand to simulate refill spacing, hold budgets and drain
/// ticks — mocked passage of time, zero real sleeps anywhere (#1211 test
/// contract). Production builds never read it.
#[cfg(test)]
static CLOCK_OFFSET_MS: AtomicU64 = AtomicU64::new(0);

/// Wall-clock seam for ALL gate math (bucket takes, hold accounting, drain
/// cadence). Production reads the real clock unchanged; test builds add the
/// monotonic [`CLOCK_OFFSET_MS`] offset so time passage is fully scripted.
#[cfg(not(test))]
fn gate_now() -> Instant {
    Instant::now()
}

/// [`gate_now`] — test build.
#[cfg(test)]
fn gate_now() -> Instant {
    let off = CLOCK_OFFSET_MS.load(Ordering::Relaxed);
    Instant::now()
        .checked_add(Duration::from_millis(off))
        .expect("virtual clock offset overflow")
}

/// Longest a SEND may be held by G3 pacing before failing open.
///
/// Sends carry real content and are delay-never-drop (#297), so the pacer has
/// no drop path at all — but an unbounded hold would pile caller tasks up
/// without limit under a pathological config or a flood-banned chat. Failing
/// open hands the send to the reactive backstop ([`super::rate_limit`]),
/// which owns oversized windows from there (#1064).
const SEND_MAX_HOLD: Duration = Duration::from_secs(30);

/// Retry budget for one queued final edit before it is abandoned with a warn.
///
/// Finals are never dropped AT ADMISSION, but a payload Telegram permanently
/// refuses (message deleted, chat gone) must not wedge the queue forever:
/// each drain attempt consumes one unit of this budget, and the reactive
/// error paths plus the next differing-content refresh take over afterwards.
const FINAL_MAX_ATTEMPTS: u32 = 8;

/// Drain-loop spacing. One queued final per tick keeps drained edits roughly
/// on the edit bucket's cadence without a dedicated wakeup channel.
const DRAIN_TICK: Duration = Duration::from_millis(400);

// ---------------------------------------------------------------------------
// Config snapshot
// ---------------------------------------------------------------------------

/// Live knob snapshot for one gate evaluation, from
/// `[channels.telegram.rate_limiter]`. Read fresh per call so config.toml
/// edits land without a restart (the same contract as `Config::current`).
struct Limits {
    enabled: bool,
    /// G1 refill spacing (one typing action per this much wall time).
    typing_interval: Duration,
    /// G1 burst capacity.
    typing_burst: u32,
    /// G1 longest hold before a typing refresh is dropped.
    typing_max_hold: Duration,
    /// G2 refill rate in tokens (edits) per second.
    edit_rate_per_sec: f64,
    /// G2 burst capacity.
    edit_burst: u32,
    /// G3 minimum spacing between sends.
    send_interval: Duration,
    /// G3 minute-ceiling capacity (refills fully over 60 s).
    send_minute_ceiling: u32,
    /// G3 burst capacity.
    send_burst: u32,
    /// Spacing of the telemetry summary INFO line.
    summary_log_period: Duration,
}

impl Limits {
    fn from_config() -> Self {
        let rl = &Config::current().channels.telegram.rate_limiter;
        let ceiling = rl.sends_ceiling_per_minute.max(1);
        Self {
            enabled: rl.enabled,
            typing_interval: Duration::from_secs(rl.typing_min_interval_secs.max(1)),
            typing_burst: rl.typing_burst.max(1),
            typing_max_hold: Duration::from_secs(rl.typing_max_hold_secs),
            edit_rate_per_sec: (rl.edits_per_minute.max(1) as f64) / 60.0,
            edit_burst: rl.edit_burst.max(1),
            send_interval: Duration::from_millis(rl.send_min_interval_millis.max(50)),
            send_minute_ceiling: ceiling,
            send_burst: rl.sends_burst.max(1),
            summary_log_period: Duration::from_secs(rl.summary_log_secs.max(30)),
        }
    }
}

// ---------------------------------------------------------------------------
// Token bucket
// ---------------------------------------------------------------------------

/// Minimal continuous-refill token bucket. Pure with respect to the injected
/// `now`, so the admission math is unit-testable without tokio or config.
struct Bucket {
    tokens: f64,
    capacity: f64,
    refill_per_sec: f64,
    last_refill: Instant,
}

impl Bucket {
    fn new(capacity: u32, refill_per_sec: f64) -> Self {
        Self {
            tokens: f64::from(capacity),
            capacity: f64::from(capacity),
            refill_per_sec,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.last_refill);
        self.last_refill = now;
        let gained = elapsed.as_secs_f64() * self.refill_per_sec;
        self.tokens = (self.tokens + gained).min(self.capacity);
    }

    /// Consume one token, or report how long until the next refills.
    fn take(&mut self, now: Instant) -> Result<(), Duration> {
        let wait = self.next_token_in(now);
        if wait.is_zero() {
            self.tokens -= 1.0;
            Ok(())
        } else {
            Err(wait)
        }
    }

    /// Peek without consuming: when the next token becomes available.
    fn next_token_in(&mut self, now: Instant) -> Duration {
        self.refill(now);
        if self.tokens >= 1.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64((1.0 - self.tokens) / self.refill_per_sec)
        }
    }
}

/// Initialize or reshape a lazily-built bucket slot. A config change that
/// moves capacity or rate rebuilds the bucket fresh (burst restored) rather
/// than silently keeping the old shape for the life of the process.
fn ensure_bucket(slot: &mut Option<Bucket>, capacity: u32, refill_per_sec: f64) -> &mut Bucket {
    let stale = match slot {
        Some(b) => b.capacity != f64::from(capacity)
            || (b.refill_per_sec - refill_per_sec).abs() > f64::EPSILON,
        None => true,
    };
    if stale {
        *slot = Some(Bucket::new(capacity, refill_per_sec));
    }
    slot.as_mut().expect("bucket was just built")
}

// ---------------------------------------------------------------------------
// Per-peer state
// ---------------------------------------------------------------------------

/// A final edit Telegram refused transiently, held latest-wins per message id
/// until the drainer can land it.
#[derive(Clone)]
struct PendingFinal {
    bot: Bot,
    html: String,
    /// Rich-API edit when true, classic HTML `editMessageText` otherwise.
    rich: bool,
    attempts: u32,
}

/// Counters behind the periodic summary line. Field-per-class instead of a
/// map so the summary formatting cannot silently miss a newly named class.
#[derive(Default)]
struct Counters {
    admitted_typing: u64,
    admitted_edits: u64,
    admitted_sends: u64,
    dropped_typing: u64,
    dropped_clock: u64,
    dropped_brain_preview: u64,
    dropped_intermediary: u64,
    dropped_status: u64,
    queued_finals: u64,
    superseded_finals: u64,
    delivered_finals: u64,
    failed_finals: u64,
    throttled_typing_ms: u64,
    throttled_send_ms: u64,
}

impl Counters {
    /// Record a ladder drop. The Final arm stays empty ON PURPOSE: finals are
    /// never dropped here — admission queues them instead (#1211).
    fn note_drop(&mut self, class: EditClass) {
        match class {
            EditClass::Clock => self.dropped_clock += 1,
            EditClass::BrainPreview => self.dropped_brain_preview += 1,
            EditClass::Intermediary => self.dropped_intermediary += 1,
            EditClass::Status => self.dropped_status += 1,
            EditClass::Final => {}
        }
    }

    /// True while nothing at all was counted — quiet forums stay silent in
    /// the periodic summary instead of logging zero-lines forever.
    fn all_zero(&self) -> bool {
        self.admitted_typing == 0
            && self.admitted_edits == 0
            && self.admitted_sends == 0
            && self.dropped_typing == 0
            && self.dropped_clock == 0
            && self.dropped_brain_preview == 0
            && self.dropped_intermediary == 0
            && self.dropped_status == 0
            && self.queued_finals == 0
            && self.superseded_finals == 0
            && self.delivered_finals == 0
            && self.failed_finals == 0
            && self.throttled_typing_ms == 0
            && self.throttled_send_ms == 0
    }
}

/// Everything the governors track for one forum peer.
#[derive(Default)]
struct Peer {
    /// Flips true the first time ANY call for this chat is observed carrying
    /// a topic id. Until then the peer passes through ungoverned: forums-only
    /// rollout, DMs (positive ids) never governed at all.
    forum_seen: bool,
    typing: Option<Bucket>,
    edits: Option<Bucket>,
    sends_sec: Option<Bucket>,
    sends_min: Option<Bucket>,
    /// Queued finals keyed by message id — insert-replace IS latest-wins.
    finals: HashMap<i32, PendingFinal>,
    /// A drainer task is currently running for this peer.
    draining: bool,
    counters: Counters,
}

fn peers() -> &'static Mutex<HashMap<i64, Peer>> {
    static PEERS: OnceLock<Mutex<HashMap<i64, Peer>>> = OnceLock::new();
    PEERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Format one peer's summary line. Pure so the field coverage is pinned by a
/// test: adding a counter without extending this format fails the test.
fn format_summary(chat_id: i64, c: &Counters, finals_pending: usize) -> Option<String> {
    if c.all_zero() && finals_pending == 0 {
        return None;
    }
    Some(format!(
        "Telegram rate-limiter chat={chat_id}: \
         admitted{{typing={},edits={},sends={}}} \
         dropped{{clock={},brain_preview={},intermediary={},status={},typing={}}} \
         finals{{queued={},superseded={},delivered={},failed={},pending={}}} \
         throttled_ms{{typing={},send={}}}",
        c.admitted_typing,
        c.admitted_edits,
        c.admitted_sends,
        c.dropped_clock,
        c.dropped_brain_preview,
        c.dropped_intermediary,
        c.dropped_status,
        c.dropped_typing,
        c.queued_finals,
        c.superseded_finals,
        c.delivered_finals,
        c.failed_finals,
        finals_pending,
        c.throttled_typing_ms,
        c.throttled_send_ms,
    ))
}

/// One summary INFO line per active forum every `[rate_limiter].
/// summary_log_secs`. Spawned lazily by the first gated call.
async fn summary_loop() {
    loop {
        let period = Limits::from_config().summary_log_period;
        tokio::time::sleep(period).await;
        let lines: Vec<String> = {
            let map = peers().lock().unwrap_or_else(|e| e.into_inner());
            map.iter()
                .filter_map(|(chat_id, peer)| {
                    format_summary(
                        *chat_id,
                        &peer.counters,
                        peer.finals.len(),
                    )
                })
                .collect()
        };
        for line in lines {
            tracing::info!("{line}");
        }
    }
}

fn ensure_summary_task() {
    // Tests run under tokio's paused clock: the periodic summary sleep would
    // auto-advance instantly and busy-spin the registry lock between virtual
    // refills. The summary format is pinned by its own unit test instead.
    #[cfg(test)]
    {
        let _ = Limits::from_config();
        return;
    }
    #[cfg(not(test))]
    {
        static STARTED: OnceLock<()> = OnceLock::new();
        STARTED.get_or_init(|| {
            tokio::spawn(summary_loop());
        });
    }
}

// ---------------------------------------------------------------------------
// G1 — typing
// ---------------------------------------------------------------------------

enum Decision {
    Admit,
    Drop,
    Hold(Duration),
}

/// G1 gate for `sendChatAction`. Returns true when the caller must fire the
/// action now; false when the refresh was collapsed away (cosmetic loss, the
/// next tick re-fires). Hold-and-release under pressure: a caller whose wait
/// still fits the hold budget sleeps for the refill window instead of
/// retry-spinning into the same bucket, which is what amplified 429 storms.
pub(crate) async fn admit_chat_action(chat: ChatId, thread_id: Option<i32>) -> bool {
    let chat_id = chat.0;
    // Positive ids are DMs — untouched by construction, no matter what a
    // future call site passes. Cheap exit before touching config.
    if chat_id >= 0 {
        return true;
    }
    let lim = Limits::from_config();
    if !lim.enabled {
        return true;
    }
    ensure_summary_task();
    let mut waited = Duration::ZERO;
    loop {
        let decision = {
            let mut map = peers().lock().unwrap_or_else(|e| e.into_inner());
            let peer = map.entry(chat_id).or_default();
            if thread_id.is_some() {
                peer.forum_seen = true;
            }
            if !peer.forum_seen {
                return true;
            }
            let bucket = ensure_bucket(
                &mut peer.typing,
                lim.typing_burst,
                1.0 / lim.typing_interval.as_secs_f64(),
            );
            match bucket.take(gate_now()) {
                Ok(()) => {
                    peer.counters.admitted_typing += 1;
                    Decision::Admit
                }
                Err(wait) => {
                    if waited + wait > lim.typing_max_hold {
                        peer.counters.dropped_typing += 1;
                        Decision::Drop
                    } else {
                        Decision::Hold(wait)
                    }
                }
            }
        };
        match decision {
            Decision::Admit => {
                fold_throttle_ms(chat_id, waited);
                return true;
            }
            Decision::Drop => {
                tracing::debug!(
                    "Telegram rate-limiter: typing refresh dropped for chat={chat_id} after \
                     holding {waited:?} (hold cap {}s)",
                    lim.typing_max_hold.as_secs()
                );
                fold_throttle_ms(chat_id, waited);
                return false;
            }
            Decision::Hold(wait) => {
                let start = gate_now();
                tokio::time::sleep(wait).await;
                waited += gate_now().duration_since(start);
            }
        }
    }
}

/// Attribute hold time spent by a gate back to its peer's counters. Best
/// effort: a peer entry vanishing between hold and fold loses one sample.
fn fold_throttle_ms(chat_id: i64, waited: Duration) {
    if waited.is_zero() {
        return;
    }
    let ms = waited.as_millis() as u64;
    if ms == 0 {
        return;
    }
    if let Some(peer) = peers().lock().unwrap_or_else(|e| e.into_inner()).get_mut(&chat_id) {
        peer.counters.throttled_typing_ms += ms;
    }
}

// ---------------------------------------------------------------------------
// G2 — edits
// ---------------------------------------------------------------------------

/// Which flow-surface edit is asking to go out. Discriminant order IS the
/// drop ladder from #1211: on an empty edit bucket, lower ranks drop first
/// because they self-heal on the next full-state refresh; finals queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditClass {
    /// Elapsed-clock chrome ticks — pure cosmetics, re-rendered next tick.
    Clock,
    /// Streaming/thinking preview churn (the ▋ cursor placeholder).
    BrainPreview,
    /// Mid-turn tool/narration appends folded into the processing log.
    Intermediary,
    /// Live status render of the open flow block (open/refresh on activity).
    Status,
    /// Settle renders and plan-card refreshes — NEVER dropped, queued
    /// latest-wins per message id until the edit bucket refills.
    Final,
}

impl EditClass {
    /// Ladder rank: higher drops later. Kept explicit so a test pins the
    /// ordering the issue locks in, even if variants reorder.
    fn drop_rank(self) -> u8 {
        match self {
            EditClass::Clock => 0,
            EditClass::BrainPreview => 1,
            EditClass::Intermediary => 2,
            EditClass::Status => 3,
            EditClass::Final => 4,
        }
    }
}

enum Admission {
    /// Token consumed — the caller performs its edit now.
    Now,
    /// Chrome class with no budget — dropped and counted (self-healing).
    Dropped(EditClass),
    /// Final with no budget — payload queued latest-wins; drainer owns it.
    Queued,
}

/// G2 gate for `editMessageText`. Returns true when the caller must perform
/// its edit now; false when the governor handled it (dropped chrome, or the
/// payload was queued as a final). `html`/`rich` describe the payload so a
/// queued final can be executed verbatim by the drainer without the caller
/// staying alive.
pub(crate) async fn edit_admission(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    class: EditClass,
    html: String,
    rich: bool,
) -> bool {
    // DMs untouched (positive ids), matching the G1 scope guard.
    if chat_id.0 >= 0 {
        return true;
    }
    let lim = Limits::from_config();
    if !lim.enabled {
        return true;
    }
    ensure_summary_task();
    let now = gate_now();
    let admission = {
        let mut map = peers().lock().unwrap_or_else(|e| e.into_inner());
        let peer = map.entry(chat_id.0).or_default();
        // Edits rarely carry a topic id; a peer reached here without any
        // topic-scoped call yet (e.g. right after restart) stays ungoverned
        // until G1 or a topic-scoped call marks it.
        if !peer.forum_seen {
            return true;
        }
        let bucket = ensure_bucket(&mut peer.edits, lim.edit_burst, lim.edit_rate_per_sec);
        if bucket.take(now).is_ok() {
            peer.counters.admitted_edits += 1;
            Admission::Now
        } else if class == EditClass::Final {
            let superseded = peer
                .finals
                .insert(
                    msg_id.0,
                    PendingFinal {
                        bot: bot.clone(),
                        html,
                        rich,
                        attempts: 0,
                    },
                )
                .is_some();
            if superseded {
                peer.counters.superseded_finals += 1;
            }
            peer.counters.queued_finals += 1;
            Admission::Queued
        } else {
            peer.counters.note_drop(class);
            Admission::Dropped(class)
        }
    };
    match admission {
        Admission::Now => true,
        Admission::Dropped(dropped) => {
            tracing::debug!(
                "Telegram rate-limiter: {:?} edit (rank {}) dropped for chat={} msg={} — \
                 next full-state refresh carries it",
                dropped,
                dropped.drop_rank(),
                chat_id.0,
                msg_id.0
            );
            false
        }
        Admission::Queued => {
            tracing::debug!(
                "Telegram rate-limiter: final edit queued latest-wins for chat={} msg={}",
                chat_id.0,
                msg_id.0
            );
            ensure_drain(chat_id.0);
            false
        }
    }
}

/// Take ownership of one queued final when the edit bucket allows it.
/// Returns `None` on "nothing to do this tick" (no peer, empty queue, or no
/// edit budget yet).
fn take_due_final(chat_id: i64, lim: &Limits) -> Option<(i32, PendingFinal)> {
    let mut map = peers().lock().unwrap_or_else(|e| e.into_inner());
    let peer = map.get_mut(&chat_id)?;
    if peer.finals.is_empty() {
        peer.draining = false;
        return None;
    }
    let bucket = ensure_bucket(&mut peer.edits, lim.edit_burst, lim.edit_rate_per_sec);
    if bucket.take(gate_now()).is_err() {
        return None;
    }
    let key = peer.finals.keys().next().copied()?;
    peer.finals.remove(&key).map(|pending| (key, pending))
}

/// Flush queued finals for one peer as the edit bucket refills. Exits when
/// the queue runs dry (clearing `draining` so a later enqueue respawns it).
async fn drain_finals(chat_id: i64) {
    loop {
        tokio::time::sleep(DRAIN_TICK).await;
        let lim = Limits::from_config();
        let Some(job) = take_due_final(chat_id, &lim) else {
            // Queue drained (draining flag cleared inside) or no budget yet.
            let empty = peers()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&chat_id)
                .is_none_or(|p| p.finals.is_empty());
            if empty {
                return;
            }
            continue;
        };
        deliver_final(chat_id, job.0, job.1).await;
    }
}

/// Respawn the drainer for a peer exactly when a first final appears and no
/// drainer is running; the task itself retires when the queue drains.
fn ensure_drain(chat_id: i64) {
    let should_spawn = {
        let mut map = peers().lock().unwrap_or_else(|e| e.into_inner());
        let peer = map.entry(chat_id).or_default();
        if peer.draining || peer.finals.is_empty() {
            false
        } else {
            peer.draining = true;
            true
        }
    };
    if should_spawn {
        tokio::spawn(drain_finals(chat_id));
    }
}

/// Errors that mean a queued final will NEVER land, no matter how often the
/// drainer retries: the target is gone or the content is already there.
fn is_permanent_edit_error(error: &str) -> bool {
    error.contains("message to edit not found")
        || error.contains("message is not modified")
        || error.contains("message can't be edited")
        || error.contains("MESSAGE_ID_INVALID")
        || error.contains("chat not found")
}

enum Verdict {
    Delivered,
    Abandoned(String),
    Retried,
}

/// Execute one queued final outside the registry lock, then record the
/// outcome. Transient failures requeue with their attempt budget decremented;
/// permanent ones abandon with a warn so a dead target cannot wedge the
/// queue forever (finals are never dropped at ADMISSION — wire failures are
/// a different failure mode with their own telemetry).
async fn deliver_final(chat_id: i64, msg_id: i32, mut pending: PendingFinal) {
    let result = run_final_edit(&pending.bot, chat_id, msg_id, &pending.html, pending.rich).await;
    let retry_after = match &result {
        Err(e) => super::rate_limit::parse_retry_after(e),
        Ok(()) => None,
    };
    let verdict = {
        let mut map = peers().lock().unwrap_or_else(|e| e.into_inner());
        let Some(peer) = map.get_mut(&chat_id) else {
            return;
        };
        match result {
            Ok(()) => {
                peer.counters.delivered_finals += 1;
                Verdict::Delivered
            }
            Err(e) => {
                pending.attempts += 1;
                if pending.attempts >= FINAL_MAX_ATTEMPTS || is_permanent_edit_error(&e) {
                    peer.counters.failed_finals += 1;
                    Verdict::Abandoned(e)
                } else {
                    peer.finals.insert(msg_id, pending);
                    Verdict::Retried
                }
            }
        }
    };
    match verdict {
        Verdict::Delivered => {
            tracing::debug!(
                "Telegram rate-limiter: queued final delivered chat={chat_id} msg={msg_id}"
            );
        }
        Verdict::Abandoned(e) => {
            tracing::warn!(
                "Telegram rate-limiter: queued final abandoned after {FINAL_MAX_ATTEMPTS} \
                 attempts chat={chat_id} msg={msg_id}: {e}"
            );
        }
        Verdict::Retried => {
            // Respect a 429 window if that is what bounced us, capped inline
            // like every other send path (#1064), then let the tick cadence
            // handle plain transients.
            if let Some(window) = retry_after {
                let (wait, _) = super::rate_limit::clamp_inline_wait(window);
                tokio::time::sleep(wait).await;
            }
        }
    }
}

/// The two wire shapes a queued final can take, mirroring the call sites that
/// produced the payload (rich API vs classic HTML edit).
async fn run_final_edit(
    bot: &Bot,
    chat_id: i64,
    msg_id: i32,
    html: &str,
    rich: bool,
) -> Result<(), String> {
    if rich {
        super::rich::api::edit_rich_html(
            bot.api_url().as_str(),
            bot.token(),
            chat_id,
            msg_id,
            html,
            None,
            "turn",
            "-",
        )
        .await
        .map_err(|e| e.to_string())
    } else {
        bot.edit_message_text(ChatId(chat_id), MessageId(msg_id), html)
            .parse_mode(ParseMode::Html)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// G3 — send pacing
// ---------------------------------------------------------------------------

enum PaceVerdict {
    Go,
    /// Budget exhausted — fail OPEN (delay-never-drop, #297): the send goes
    /// out and the reactive backstop owns whatever comes back.
    FailOpen(Duration),
    Wait(Duration),
}

/// G3 gate ahead of full-message sends. Two AND-ed buckets: ~1/s spacing and
/// an ~18/min group ceiling (both configurable). Holds the caller just long
/// enough to buy a token pair, never drops, and fails open past
/// [`SEND_MAX_HOLD`] so a pathological configuration degrades to today's
/// behavior instead of stalling turns.
pub(crate) async fn pace_send(chat: ChatId) {
    let chat_id = chat.0;
    // DMs (positive ids) untouched, cheap exit first.
    if chat_id >= 0 {
        return;
    }
    let lim = Limits::from_config();
    if !lim.enabled {
        return;
    }
    let mut waited = Duration::ZERO;
    loop {
        let verdict = {
            let mut map = peers().lock().unwrap_or_else(|e| e.into_inner());
            let peer = map.entry(chat_id).or_default();
            if !peer.forum_seen {
                return;
            }
            let sec = ensure_bucket(
                &mut peer.sends_sec,
                lim.send_burst,
                1.0 / lim.send_interval.as_secs_f64(),
            );
            let min = ensure_bucket(
                &mut peer.sends_min,
                lim.send_minute_ceiling,
                f64::from(lim.send_minute_ceiling) / 60.0,
            );
            let now = gate_now();
            let need = sec.next_token_in(now).max(min.next_token_in(now));
            if need.is_zero() {
                let _ = sec.take(now);
                let _ = min.take(now);
                peer.counters.admitted_sends += 1;
                PaceVerdict::Go
            } else if waited + need > SEND_MAX_HOLD {
                peer.counters.admitted_sends += 1;
                PaceVerdict::FailOpen(need)
            } else {
                PaceVerdict::Wait(need)
            }
        };
        match verdict {
            PaceVerdict::Go => {
                fold_send_ms(chat_id, waited);
                return;
            }
            PaceVerdict::FailOpen(next) => {
                tracing::warn!(
                    "Telegram rate-limiter: send pacing held {waited:?} for chat={chat_id}, \
                     still {} from a token — failing open to the reactive backstop",
                    next.as_secs_f64()
                );
                fold_send_ms(chat_id, waited);
                return;
            }
            PaceVerdict::Wait(delay) => {
                let start = gate_now();
                tokio::time::sleep(delay).await;
                waited += gate_now().duration_since(start);
            }
        }
    }
}

/// Attribute G3 hold time back to the peer's counters. Best effort, mirroring
/// [`fold_throttle_ms`].
fn fold_send_ms(chat_id: i64, waited: Duration) {
    if waited.is_zero() {
        return;
    }
    let ms = waited.as_millis() as u64;
    if ms == 0 {
        return;
    }
    if let Some(peer) = peers().lock().unwrap_or_else(|e| e.into_inner()).get_mut(&chat_id) {
        peer.counters.throttled_send_ms += ms;
    }
}

// ---------------------------------------------------------------------------
// Test support (cfg(test)) — shared with src/tests/governor_gates_test.rs
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    use std::sync::atomic::Ordering;

    /// Serialize every test that touches the global peer registry, the
    /// virtual clock or the process-wide config mirror. The gates are built
    /// on process singletons by design (one bot token per instance), so the
    /// tests that exercise them take this guard first.
    pub(crate) async fn registry_guard() -> tokio::sync::MutexGuard<'static, ()> {
        static GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        GUARD.lock().await
    }

    /// Wipe all peer state and pin the virtual clock at `offset_ms`.
    pub(crate) fn reset(offset_ms: u64) {
        peers().lock().unwrap_or_else(|e| e.into_inner()).clear();
        CLOCK_OFFSET_MS.store(offset_ms, Ordering::Relaxed);
    }

    /// Advance the virtual clock: bucket refills, hold budgets and drain
    /// cadence observe the jump on their next `gate_now` evaluation. No real
    /// sleeping anywhere — mocked passage of time per the #1211 test brief.
    pub(crate) fn advance(ms: u64) {
        CLOCK_OFFSET_MS.fetch_add(ms, Ordering::Relaxed);
    }

    /// Mark a chat as an already-seen forum peer WITHOUT consuming budget,
    /// so a test controls exactly how many tokens each bucket starts with.
    pub(crate) fn mark_forum(chat: ChatId) {
        let chat_id = chat.0;
        peers()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(chat_id)
            .or_default()
            .forum_seen = true;
    }

    /// Empty one of a peer's buckets through the SAME refill/take math the
    /// gates use (`kind`: "typing" | "edits" | "sends_sec" | "sends_min").
    pub(crate) fn burn_bucket(chat: ChatId, kind: BucketKind, capacity: u32, rate_per_sec: f64) {
        let chat_id = chat.0;
        let mut map = peers().lock().unwrap_or_else(|e| e.into_inner());
        let peer = map.entry(chat_id).or_default();
        peer.forum_seen = true;
        let slot = match kind {
            BucketKind::Typing => &mut peer.typing,
            BucketKind::Edits => &mut peer.edits,
            BucketKind::SendsSec => &mut peer.sends_sec,
            BucketKind::SendsMin => &mut peer.sends_min,
        };
        let bucket = ensure_bucket(slot, capacity, rate_per_sec);
        for _ in 0..capacity {
            let _ = bucket.take(gate_now());
        }
    }

    /// Which of a peer's buckets [`burn_bucket`] empties.
    #[derive(Debug, Clone, Copy)]
    pub(crate) enum BucketKind {
        Typing,
        Edits,
        SendsSec,
        SendsMin,
    }

    /// Point-in-time copy of everything a test can assert on for one peer.
    #[derive(Debug, Default, PartialEq, Eq)]
    pub(crate) struct Snap {
        pub forum_seen: bool,
        pub typing_admitted: u64,
        pub typing_dropped: u64,
        pub edits_admitted: u64,
        pub dropped_clock: u64,
        pub dropped_brain_preview: u64,
        pub dropped_intermediary: u64,
        pub dropped_status: u64,
        pub queued_finals: u64,
        pub superseded_finals: u64,
        pub delivered_finals: u64,
        pub failed_finals: u64,
        pub throttled_typing_ms: u64,
        pub throttled_send_ms: u64,
        pub finals_pending: usize,
    }

    /// Read [`Snap`] for `chat_id`; `None` when no gate has touched the peer.
    pub(crate) fn snapshot(chat: ChatId) -> Option<Snap> {
        let chat_id = chat.0;
        let map = peers().lock().unwrap_or_else(|e| e.into_inner());
        map.get(&chat_id).map(|p| Snap {
            forum_seen: p.forum_seen,
            typing_admitted: p.counters.admitted_typing as u64,
            typing_dropped: p.counters.dropped_typing as u64,
            edits_admitted: p.counters.admitted_edits as u64,
            dropped_clock: p.counters.dropped_clock as u64,
            dropped_brain_preview: p.counters.dropped_brain_preview as u64,
            dropped_intermediary: p.counters.dropped_intermediary as u64,
            dropped_status: p.counters.dropped_status as u64,
            queued_finals: p.counters.queued_finals as u64,
            superseded_finals: p.counters.superseded_finals as u64,
            delivered_finals: p.counters.delivered_finals as u64,
            failed_finals: p.counters.failed_finals as u64,
            throttled_typing_ms: p.counters.throttled_typing_ms as u64,
            throttled_send_ms: p.counters.throttled_send_ms as u64,
            finals_pending: p.finals.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_allows_burst_then_throttles() {
        let mut b = Bucket::new(3, 1.0);
        let t0 = Instant::now();
        assert!(b.take(t0).is_ok());
        assert!(b.take(t0).is_ok());
        assert!(b.take(t0).is_ok());
        // Empty: refuses, and reports the full refill spacing.
        let err = b.take(t0).expect_err("bucket should be empty");
        assert_eq!(err, Duration::from_secs(1));
    }

    #[test]
    fn bucket_refills_over_time_and_caps_at_capacity() {
        let mut b = Bucket::new(2, 1.0);
        let t0 = Instant::now();
        assert!(b.take(t0).is_ok());
        assert!(b.take(t0).is_ok());
        assert!(b.take(t0).is_err());
        // Halfway through one interval there is still no full token.
        assert!(b.take(t0 + Duration::from_millis(500)).is_err());
        // A full interval later the token is back.
        assert!(b.take(t0 + Duration::from_secs(1)).is_ok());
        // Idling far past capacity must not hoard tokens beyond the cap.
        let t1 = t0 + Duration::from_secs(60);
        assert!(b.take(t1).is_ok());
        assert!(b.take(t1).is_ok());
        assert!(b.take(t1).is_err());
    }

    #[test]
    fn ensure_bucket_rebuilds_on_shape_change_keeps_on_match() {
        let mut slot = None;
        let rate = 0.5;
        ensure_bucket(&mut slot, 10, rate).take(Instant::now()).ok();
        // Same shape: the partially-consumed bucket survives.
        let kept = ensure_bucket(&mut slot, 10, rate);
        assert!(kept.tokens < f64::from(10u32));
        // Different shape: rebuilt fresh at full capacity.
        let fresh = ensure_bucket(&mut slot, 5, rate);
        assert!((fresh.tokens - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ladder_order_drops_clock_first_and_final_never_drops() {
        let ladder = [
            EditClass::Clock,
            EditClass::BrainPreview,
            EditClass::Intermediary,
            EditClass::Status,
        ];
        for pair in ladder.windows(2) {
            assert!(
                pair[0].drop_rank() < pair[1].drop_rank(),
                "ladder must drop {pair:?} in ascending value order"
            );
        }
        // Final outranks everything and is refused by the dropper.
        assert_eq!(EditClass::Final.drop_rank(), 4);
        let mut c = Counters::default();
        c.note_drop(EditClass::Final);
        assert_eq!(c.dropped_clock + c.dropped_status, 0, "finals are never dropped");
    }

    #[test]
    fn note_drop_counts_each_chrome_class() {
        let mut c = Counters::default();
        c.note_drop(EditClass::Clock);
        c.note_drop(EditClass::Clock);
        c.note_drop(EditClass::BrainPreview);
        c.note_drop(EditClass::Intermediary);
        c.note_drop(EditClass::Status);
        assert_eq!(c.dropped_clock, 2);
        assert_eq!(c.dropped_brain_preview, 1);
        assert_eq!(c.dropped_intermediary, 1);
        assert_eq!(c.dropped_status, 1);
    }

    #[test]
    fn summary_is_silent_when_idle() {
        let c = Counters::default();
        assert!(format_summary(-100123, &c, 0).is_none());
    }

    #[test]
    fn summary_carries_every_counter_group() {
        let mut c = Counters::default();
        c.admitted_typing = 12;
        c.admitted_edits = 34;
        c.admitted_sends = 5;
        c.dropped_clock = 1;
        c.dropped_brain_preview = 2;
        c.dropped_intermediary = 3;
        c.dropped_status = 4;
        c.dropped_typing = 6;
        c.queued_finals = 7;
        c.superseded_finals = 8;
        c.delivered_finals = 9;
        c.failed_finals = 10;
        c.throttled_typing_ms = 1500;
        c.throttled_send_ms = 2500;
        let line = format_summary(-100123, &c, 2).expect("active peer must summarize");
        assert!(line.contains("chat=-100123"));
        assert!(line.contains("admitted{typing=12,edits=34,sends=5}"));
        assert!(line.contains("dropped{clock=1,brain_preview=2,intermediary=3,status=4,typing=6}"));
        assert!(line.contains("finals{queued=7,superseded=8,delivered=9,failed=10,pending=2}"));
        assert!(line.contains("throttled_ms{typing=1500,send=2500}"));
    }

    #[test]
    fn permanent_edit_error_vocabulary_is_exact() {
        assert!(is_permanent_edit_error(
            "Telegram error: message to edit not found"
        ));
        assert!(is_permanent_edit_error("Bad Request: message is not modified"));
        assert!(!is_permanent_edit_error("Too Many Requests: retry after 3"));
        assert!(!is_permanent_edit_error("timeout"));
    }
}
