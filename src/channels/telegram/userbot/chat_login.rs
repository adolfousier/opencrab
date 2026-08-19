//! Chat-driven userbot login — the Telegram-native `/userbot-login` flow.
//!
//! One bounded state machine (a single concurrent flow per bot), driven by
//! the owner's messages: collect credentials (positional or unordered),
//! persist them to keys.toml, then run the QR login with the QR rendered
//! into the chat and the 2FA cloud password collected conversationally.
//!
//! Invariants: `api_hash` and the 2FA password are never echoed; the flow
//! expires (10 min inactivity) and cancels ('cancel'); every message that is
//! not part of an active flow falls through to the normal handler untouched.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use teloxide::Bot;
use teloxide::payloads::SendPhotoSetters as _;
use teloxide::prelude::ChatId;
use teloxide::types::{InputFile, ThreadId};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::UserbotCreds;
use super::login::{QrStep, connect, finish, password_step, qr_poll_once};
use super::setup::{
    CollectedCredentials, CredentialDraft, parse_login_command, persist_credentials,
};
use super::{TelegramUserbotConfig, resolve_creds};
use crate::channels::telegram::send::{message_in_thread, photo_in_thread};
use crate::config::types::Config;

const FLOW_TIMEOUT: Duration = Duration::from_secs(600);
const PASSWORD_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChatRef {
    pub(crate) chat_id: i64,
    pub(crate) thread_id: Option<ThreadId>,
}

enum Stage {
    CollectingCreds(CredentialDraft),
    /// Login task running (QR polling / verifying).
    QrFlow,
    /// Login task parked, waiting for the owner's 2FA password.
    AwaitingPassword(oneshot::Sender<String>),
}

struct Pending {
    id: u64,
    user_id: i64,
    chat: ChatRef,
    stage: Stage,
    deadline: Instant,
    cancel: CancellationToken,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

fn lock_pending() -> std::sync::MutexGuard<'static, Option<Pending>> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner())
}

fn is_cancel(text: &str) -> bool {
    text.trim().eq_ignore_ascii_case("cancel")
}

/// Cheap gate for the message handler: is this the command, or a follow-up
/// message inside an active flow owned by this user in this chat?
pub(crate) fn wants(text: &str, user_id: i64, chat_id: i64) -> bool {
    if parse_login_command(text).is_some() {
        return true;
    }
    lock_pending()
        .as_ref()
        .is_some_and(|p| p.user_id == user_id && p.chat.chat_id == chat_id)
}

/// Handle a message for the flow. Returns `true` when the message was
/// consumed (the caller must not forward it to the agent).
pub(crate) async fn intercept(
    bot: &Bot,
    chat: ChatRef,
    user_id: i64,
    is_private: bool,
    text: &str,
    cfg: &Config,
) -> Result<bool> {
    let text = text.trim();
    let mut replies: Vec<String> = Vec::new();
    let mut start: Option<CollectedCredentials> = None;
    let mut warn_group = false;

    // ── State decisions under the lock; no I/O inside ────────────────────
    {
        let mut guard = lock_pending();

        // Expire stale flows so a forgotten prompt never swallows messages.
        if let Some(p) = guard.as_ref()
            && p.deadline < Instant::now()
        {
            let expired_here = p.chat == chat && p.user_id == user_id;
            let cancel = p.cancel.clone();
            *guard = None;
            cancel.cancel();
            if expired_here {
                replies.push("⌛️ Userbot login expired — /userbot-login to restart.".into());
            }
        }

        if let Some(args) = parse_login_command(text) {
            // ── Command path (owner-only) ────────────────────────────────
            if !cfg.channels.telegram.is_owner(&user_id.to_string()) {
                replies.push("🔒 /userbot-login is owner-only.".into());
            } else {
                // A fresh command supersedes any flow already in flight.
                if let Some(old) = guard.take() {
                    old.cancel.cancel();
                }
                if args.is_empty() {
                    match resolve_creds(&cfg.channels.telegram.userbot) {
                        Ok(creds) => {
                            replies.push("🔑 Credentials found — starting QR login…".into());
                            start = Some(CollectedCredentials {
                                api_id: creds.api_id as i64,
                                api_hash: creds.api_hash,
                                phone: creds.phone,
                            });
                        }
                        Err(_) => {
                            *guard = Some(Pending {
                                id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                                user_id,
                                chat,
                                stage: Stage::CollectingCreds(CredentialDraft::default()),
                                deadline: Instant::now() + FLOW_TIMEOUT,
                                cancel: CancellationToken::new(),
                            });
                            replies.push(collect_prompt());
                        }
                    }
                } else {
                    let mut draft = CredentialDraft::default();
                    match draft.ingest_positional(args) {
                        Err(e) => replies.push(e),
                        Ok(()) => {
                            replies.push("🔐 Credentials accepted — starting QR login…".into());
                            start =
                                Some(draft.complete().expect("positional ingest fills all slots"));
                            warn_group = !is_private;
                        }
                    }
                }
            }
        } else {
            // ── Follow-up message: only meaningful inside this user's flow ─
            let owns_flow = guard
                .as_ref()
                .is_some_and(|p| p.user_id == user_id && p.chat == chat);
            if !owns_flow {
                return Ok(false);
            }
            if is_cancel(text) {
                if let Some(p) = guard.take() {
                    p.cancel.cancel();
                }
                replies.push("🦀 Userbot login cancelled.".into());
            } else {
                let Some(mut p) = guard.take() else {
                    return Ok(false);
                };
                match &mut p.stage {
                    Stage::CollectingCreds(draft) => match draft.ingest_unordered(text) {
                        Err(e) => {
                            p.deadline = Instant::now() + FLOW_TIMEOUT;
                            replies.push(e);
                            *guard = Some(p);
                        }
                        Ok(()) if draft.is_complete() => {
                            let creds = draft.clone().complete().expect("complete checked");
                            start = Some(creds);
                            replies.push("🔐 Credentials look good — starting QR login…".into());
                        }
                        Ok(()) => {
                            p.deadline = Instant::now() + FLOW_TIMEOUT;
                            replies.push(format!(
                                "Got it — still need: {}",
                                draft.missing_names().join(", ")
                            ));
                            *guard = Some(p);
                        }
                    },
                    Stage::AwaitingPassword(_) => {
                        if text.starts_with('/') {
                            // A real command passes through; the flow keeps waiting.
                            *guard = Some(p);
                            return Ok(false);
                        }
                        let Stage::AwaitingPassword(tx) =
                            std::mem::replace(&mut p.stage, Stage::QrFlow)
                        else {
                            unreachable!("stage checked by the match arm")
                        };
                        let _ = tx.send(text.to_owned());
                        p.deadline = Instant::now() + FLOW_TIMEOUT;
                        *guard = Some(p);
                        replies.push("Checking password…".into());
                    }
                    Stage::QrFlow => {
                        // QR in progress and no password expected: not ours.
                        *guard = Some(p);
                        return Ok(false);
                    }
                }
            }
        }
    }

    // ── All I/O outside the state lock ──────────────────────────────────
    if let Some(creds) = start {
        if let Err(e) = persist_credentials(&creds) {
            replies.push(format!("❌ Could not save credentials to keys.toml: {e:#}"));
        } else {
            if warn_group {
                replies.push(
                    "⚠️ The api_hash now lives in this chat's history — rotate it at \
                     my.telegram.org if that matters."
                        .into(),
                );
            }
            spawn_login(
                bot.clone(),
                chat,
                user_id,
                creds,
                cfg.channels.telegram.userbot.clone(),
            );
        }
    }
    for reply in replies {
        if let Err(e) = message_in_thread(bot, ChatId(chat.chat_id), chat.thread_id, reply).await {
            tracing::warn!("userbot login: reply send failed: {e}");
        }
    }
    Ok(true)
}

fn collect_prompt() -> String {
    "🔑 Userbot login — send your API credentials (my.telegram.org → API development \
     tools):\n• api_id — digits\n• api_hash — 32 hex characters\n• phone — like +2547…\n\n\
     All three in one message, or one at a time in any order. 'cancel' to abort."
        .into()
}

/// Register the flow's pending slot, then run the login on a background task.
fn spawn_login(
    bot: Bot,
    chat: ChatRef,
    user_id: i64,
    creds: CollectedCredentials,
    mut cfg: TelegramUserbotConfig,
) {
    cfg.api_id = Some(creds.api_id);
    cfg.api_hash = Some(creds.api_hash.clone());
    cfg.phone = Some(creds.phone.clone());
    let flow_creds = UserbotCreds {
        api_id: creds.api_id as i32,
        api_hash: creds.api_hash,
        phone: creds.phone,
    };
    let cancel = CancellationToken::new();
    let id = {
        let mut guard = lock_pending();
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        *guard = Some(Pending {
            id,
            user_id,
            chat,
            stage: Stage::QrFlow,
            deadline: Instant::now() + FLOW_TIMEOUT,
            cancel: cancel.clone(),
        });
        id
    };
    tokio::spawn(async move {
        let (name, fresh) = match run_login(&bot, chat, &cfg, &flow_creds, &cancel).await {
            Ok(out) => out,
            Err(_) if cancel.is_cancelled() => {
                report(&bot, chat, "🦀 Userbot login cancelled.".into()).await;
                clear_if(id);
                return;
            }
            Err(e) => {
                report(
                    &bot,
                    chat,
                    format!("❌ Userbot login failed: {e:#}\n/userbot-login to retry."),
                )
                .await;
                clear_if(id);
                return;
            }
        };
        let msg = if fresh {
            format!(
                "✅ Userbot authorized as {name}. Session saved (0600). Restart opencrabs — or \
                 toggle channels.telegram.userbot.enabled — to start the watch loop."
            )
        } else {
            format!("✅ Already authorized as {name} — nothing to do.")
        };
        report(&bot, chat, msg).await;
        clear_if(id);
    });
}

/// QR login driven from the chat. Returns `(display name, fresh login?)`.
async fn run_login(
    bot: &Bot,
    chat: ChatRef,
    cfg: &TelegramUserbotConfig,
    creds: &UserbotCreds,
    cancel: &CancellationToken,
) -> Result<(String, bool)> {
    let (client, session, _updates) = connect(cfg).await?;
    if client.is_authorized().await? {
        let me = client.get_me().await?;
        return Ok((me.first_name().unwrap_or("?").to_string(), false));
    }
    let mut last: Vec<u8> = Vec::new();
    let deadline = Instant::now() + FLOW_TIMEOUT;
    loop {
        anyhow::ensure!(Instant::now() < deadline, "timed out waiting for QR scan");
        let step = tokio::select! {
            _ = cancel.cancelled() => anyhow::bail!("cancelled"),
            s = qr_poll_once(&client, creds) => s?,
        };
        let auth = match step {
            QrStep::Success(auth) => *auth,
            QrStep::PasswordNeeded => {
                let pass = collect_password(bot, chat, cancel).await?;
                password_step(&client, pass).await?
            }
            QrStep::Token(t) => {
                if t != last {
                    last = t.clone();
                    send_qr(bot, chat, &t).await;
                }
                tokio::select! {
                    _ = cancel.cancelled() => anyhow::bail!("cancelled"),
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                }
                continue;
            }
        };
        let name = finish(&client, &session, auth).await?;
        // finish() only mutated in-memory state — the auth key dies with this
        // task unless persisted NOW (the watch loop's save ticker isn't up).
        session.save().context("persisting fresh session")?;
        return Ok((name, true));
    }
}

/// Park a oneshot in the pending slot so the next owner message can deliver
/// the 2FA password, then wait for it (bounded). The password is consumed
/// without ever being echoed or logged.
async fn collect_password(bot: &Bot, chat: ChatRef, cancel: &CancellationToken) -> Result<String> {
    let (tx, rx) = oneshot::channel();
    {
        let mut guard = lock_pending();
        let Some(p) = guard.as_mut() else {
            anyhow::bail!("login flow no longer active");
        };
        anyhow::ensure!(
            matches!(p.stage, Stage::QrFlow),
            "login flow is not waiting for a password"
        );
        p.stage = Stage::AwaitingPassword(tx);
        p.deadline = Instant::now() + FLOW_TIMEOUT;
    }
    report(
        bot,
        chat,
        "🔐 Your account has a 2FA cloud password — send it here (or 'cancel'). \
         It is never echoed or logged."
            .into(),
    )
    .await;
    tokio::select! {
        _ = cancel.cancelled() => anyhow::bail!("cancelled"),
        _ = tokio::time::sleep(PASSWORD_TIMEOUT) => anyhow::bail!("timed out waiting for password"),
        r = rx => r.context("password channel closed"),
    }
}

async fn send_qr(bot: &Bot, chat: ChatRef, token: &[u8]) {
    let url = format!("tg://login?token={}", URL_SAFE_NO_PAD.encode(token));
    match crate::brain::tools::whatsapp_connect::render_qr_png(&url) {
        Some(png) => {
            let req = photo_in_thread(
                bot,
                ChatId(chat.chat_id),
                chat.thread_id,
                InputFile::memory(png).file_name("login-qr.png"),
            )
            .caption(
                "📷 Scan with your phone: Telegram → Settings → Devices → Link Desktop Device.\n\
                 (valid ~3 min; a fresh QR follows automatically)",
            );
            if let Err(e) = req.await {
                tracing::warn!("userbot login: QR send failed: {e}");
            }
        }
        None => {
            report(
                bot,
                chat,
                format!("QR render failed — open on a logged-in device: {url}"),
            )
            .await;
        }
    }
}

async fn report(bot: &Bot, chat: ChatRef, text: String) {
    if let Err(e) = message_in_thread(bot, ChatId(chat.chat_id), chat.thread_id, text).await {
        tracing::warn!("userbot login: report send failed: {e}");
    }
}

fn clear_if(id: u64) {
    let mut guard = lock_pending();
    if guard.as_ref().is_some_and(|p| p.id == id) {
        *guard = None;
    }
}
