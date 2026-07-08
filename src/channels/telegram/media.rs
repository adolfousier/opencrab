//! Incoming Telegram media: caption merging, file download/saving to tmp,
//! recent-tmp-file lookup for voice/attachment turns, and the size-capped
//! getFile wrapper.
//!
//! Moved VERBATIM out of handler.rs (#471 phase 1, pure decomposition —
//! only visibility widened to pub(crate) so the handler glob re-export
//! keeps every existing call site and test import stable).

use super::send::message_in_thread;
use teloxide::prelude::*;

pub(crate) fn prepend_caption(caption: &str, body: String) -> String {
    if caption.trim().is_empty() {
        body
    } else {
        format!("{caption}\n\n{body}")
    }
}

/// Fire-and-forget: save any incoming voice/document/audio file to
/// `~/.opencrabs/tmp/` so the agent can pick them up later when tagged.
/// This runs for ALL incoming messages regardless of mention-only status.
/// Rewrite `<<IMG:local_path>>` markers in `text` to their archived location
/// under the session's project files dir. Each local image is tracked via
/// `FileService`, which copies it into `projects/<name>/files/` when the
/// session belongs to a project; for non-project sessions (or URLs) the path
/// is returned unchanged, so the marker is left as-is.
pub(crate) async fn archive_image_markers(
    text: &str,
    session_id: uuid::Uuid,
    fs: &crate::services::FileService,
) -> String {
    use std::path::PathBuf;
    let mut replacements: Vec<(String, String)> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<<IMG:") {
        let after = &rest[start + 6..];
        let Some(end) = after.find(">>") else { break };
        let path = &after[..end];
        if !path.starts_with("http")
            && let Ok(file) = fs
                .get_or_create_file(session_id, PathBuf::from(path), None)
                .await
        {
            let new = file.path.to_string_lossy().to_string();
            if new != path {
                replacements.push((path.to_string(), new));
            }
        }
        rest = &after[end + 2..];
    }
    let mut out = text.to_string();
    for (old, new) in replacements {
        out = out.replace(&format!("<<IMG:{old}>>"), &format!("<<IMG:{new}>>"));
    }
    out
}

pub(crate) async fn save_incoming_files_to_tmp(bot: &Bot, msg: &Message, bot_token: &str) {
    use std::path::PathBuf;

    // Skip private chats — the bot will process those directly
    if matches!(msg.chat.kind, teloxide::types::ChatKind::Private { .. }) {
        return;
    }

    // Profile-aware: same tmp dir save_to_temp uses, so saves and pickups
    // agree and a profile's files stay under that profile.
    let tmp_dir: PathBuf = crate::config::opencrabs_home().join("tmp");
    let _ = std::fs::create_dir_all(&tmp_dir);

    let chat_id = msg.chat.id.0;
    let ts = chrono::Utc::now().timestamp();

    // Voice messages (.ogg)
    if let Some(voice) = msg.voice() {
        save_telegram_file(
            bot,
            bot_token,
            voice.file.id.clone(),
            &tmp_dir,
            &format!("voice-{chat_id}-{ts}.ogg"),
        )
        .await;
    }
    // Video notes (.mp4)
    if let Some(vn) = msg.video_note() {
        save_telegram_file(
            bot,
            bot_token,
            vn.file.id.clone(),
            &tmp_dir,
            &format!("video_note-{chat_id}-{ts}.mp4"),
        )
        .await;
    }
    // Documents (preserve original extension)
    if let Some(doc) = msg.document() {
        let ext = doc
            .file_name
            .as_deref()
            .and_then(|n| n.rsplit('.').next())
            .unwrap_or("bin");
        save_telegram_file(
            bot,
            bot_token,
            doc.file.id.clone(),
            &tmp_dir,
            &format!("doc-{chat_id}-{ts}.{ext}"),
        )
        .await;
    }
    // Audio files (.mp3/.ogg/.wav etc)
    if let Some(audio) = msg.audio() {
        let ext = audio
            .file_name
            .as_deref()
            .and_then(|n| n.rsplit('.').next())
            .unwrap_or("ogg");
        save_telegram_file(
            bot,
            bot_token,
            audio.file.id.clone(),
            &tmp_dir,
            &format!("audio-{chat_id}-{ts}.{ext}"),
        )
        .await;
    }
    // Photos (largest size) — so an image shared without @mentioning the bot
    // can still be picked up when the user tags it in a follow-up message.
    // Telegram orders sizes smallest→largest, so the last entry is the best.
    if let Some(largest) = msg.photo().and_then(|sizes| sizes.last()) {
        save_telegram_file(
            bot,
            bot_token,
            largest.file.id.clone(),
            &tmp_dir,
            &format!("photo-{chat_id}-{ts}.jpg"),
        )
        .await;
    }
}

/// Download a file from Telegram by file_id and save to the given path.
pub(crate) async fn save_telegram_file(
    bot: &Bot,
    bot_token: &str,
    file_id: teloxide::types::FileId,
    dir: &std::path::Path,
    filename: &str,
) {
    let file = match bot.get_file(file_id).await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("Telegram: tmp save: get_file failed: {e}");
            return;
        }
    };
    let url = format!("https://api.telegram.org/file/bot{bot_token}/{}", file.path);
    let bytes = match reqwest::get(&url).await {
        Ok(r) => match r.bytes().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("Telegram: tmp save: read bytes failed: {e}");
                return;
            }
        },
        Err(e) => {
            tracing::warn!("Telegram: tmp save: download failed: {e}");
            return;
        }
    };
    let path = dir.join(filename);
    match std::fs::write(&path, &bytes) {
        Ok(()) => tracing::info!("Telegram: saved incoming file → {}", path.display()),
        Err(e) => tracing::warn!("Telegram: tmp save: write failed: {e}"),
    }
}

/// Check `~/.opencrabs/tmp/` for the most recent voice/audio file from a
/// specific chat (within `max_age_secs`). Returns the path if found.
pub(crate) fn find_recent_voice_in_tmp(
    chat_id: i64,
    max_age_secs: i64,
) -> Option<std::path::PathBuf> {
    find_recent_tmp_file(chat_id, "voice", max_age_secs)
}

/// Find the newest `~/.opencrabs/tmp/{kind}-{chat_id}-{ts}.*` file within
/// `max_age_secs`. Used to pick up a voice/photo a user sent to a mention-only
/// group *before* tagging the bot (the file was stored fire-and-forget on
/// arrival; this retrieves it when the follow-up @mention finally triggers us).
pub(crate) fn find_recent_tmp_file(
    chat_id: i64,
    kind: &str,
    max_age_secs: i64,
) -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    // Profile-aware: same tmp dir save_to_temp uses, so saves and pickups
    // agree and a profile's files stay under that profile.
    let tmp_dir: PathBuf = crate::config::opencrabs_home().join("tmp");

    let now = chrono::Utc::now().timestamp();
    let prefix = format!("{kind}-{chat_id}-");

    let mut best: Option<(i64, PathBuf)> = None;

    let entries = std::fs::read_dir(&tmp_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with(&prefix) {
            continue;
        }
        // Extract timestamp from filename: voice-{chat_id}-{ts}.ogg
        let ts_str = name_str.strip_prefix(&prefix)?.split('.').next()?;
        let ts: i64 = ts_str.parse().ok()?;
        if now - ts > max_age_secs {
            continue;
        }
        match &best {
            Some((best_ts, _)) if ts <= *best_ts => {}
            _ => best = Some((ts, entry.path())),
        }
    }
    best.map(|(_, p)| p)
}

/// Like [`find_recent_tmp_file`] but returns ALL matching files, sorted
/// oldest-first. Used for multi-photo pickup so every image the user
/// dropped is included, not just the last one.
pub(crate) fn find_all_recent_tmp_files(
    chat_id: i64,
    kind: &str,
    max_age_secs: i64,
) -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;

    let tmp_dir: PathBuf = crate::config::opencrabs_home().join("tmp");
    let now = chrono::Utc::now().timestamp();
    let prefix = format!("{kind}-{chat_id}-");

    let mut results: Vec<(i64, PathBuf)> = Vec::new();
    let entries = match std::fs::read_dir(&tmp_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with(&prefix) {
            continue;
        }
        let ts_str = match name_str
            .strip_prefix(&prefix)
            .and_then(|s| s.split('.').next())
        {
            Some(s) => s,
            None => continue,
        };
        let ts: i64 = match ts_str.parse() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if now - ts <= max_age_secs {
            results.push((ts, entry.path()));
        }
    }
    results.sort_by_key(|(ts, _)| *ts);
    results.into_iter().map(|(_, p)| p).collect()
}

/// Wrap `bot.get_file(...)` so size-limit failures (and other errors) become
/// a user-visible reply instead of a silent error in the bot logs.
///
/// Telegram's Bot API enforces a hard 20 MB cap on `getFile` even though chat
/// uploads may be much larger. When a user sends a video, animation, document,
/// or video_note that exceeds this cap the API returns
/// `Bad Request: file is too big`. Without this helper the `?` operator
/// bubbled the error up and the user heard nothing back.
///
/// Returns `Some(File)` on success, or `None` after notifying the user (caller
/// should `return Ok(())`).
pub(crate) async fn fetch_file_or_notify(
    bot: &Bot,
    file_id: teloxide::types::FileId,
    chat_id: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    kind: &str,
) -> Option<teloxide::types::File> {
    use teloxide::payloads::SendMessageSetters;
    match bot.get_file(file_id.clone()).await {
        Ok(f) => Some(f),
        Err(e) => {
            let s = e.to_string();
            let reply = if s.contains("file is too big") {
                format!(
                    "📎 That {kind} exceeds Telegram's 20 MB Bot API download limit. \
                     The chat itself accepts larger files but bots cannot fetch them. \
                     Please trim or compress the {kind} to under 20 MB and resend."
                )
            } else {
                format!("Failed to fetch {kind}: {s}")
            };
            tracing::warn!("Telegram: get_file failed for {}: {}", kind, s);
            if let Err(send_err) = message_in_thread(bot, chat_id, thread_id, reply)
                .disable_notification(true)
                .await
            {
                tracing::warn!(
                    "Telegram: failed to send size-limit reply to user: {}",
                    send_err
                );
            }
            None
        }
    }
}
