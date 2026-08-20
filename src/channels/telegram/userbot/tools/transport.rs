//! Tool transport: authorized commands → gramers calls → MCP-shaped
//! JSON envelopes. Read tools are pure queries; outbound tools
//! (send/edit/phone) mutate Telegram as the logged-in user and are
//! irreversible-class — `dispatch::authorize` gates them behind
//! `outbound_allowlist` before any code here runs.

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, FixedOffset};
use grammers_client::Client;
use grammers_client::message::{InputMessage, Message};
use grammers_session::types::{PeerAuth, PeerId, PeerRef};
use grammers_tl_types as tl;
use serde_json::{Value, json};

use super::commands::{
    Discover, EditMessage, Raw, ReadChat, SearchChat, SearchGlobal, SendFile, SendMessage,
    SendToPhone,
};
use super::mapping::{
    narrow_message_id, normalize_phone, parse_bot_api_chat_id, parse_date, truncate_with_more,
};
use super::raw;
use super::render;

/// Collect at most `$n` items from an iterator with a `next()` method,
/// stopping early on exhaustion. Macro, not a generic function: the
/// three grammers iterators share no public trait surface worth
/// depending on, and a 6-line loop beats a lifetime puzzle.
macro_rules! drain {
    ($iter:expr, $n:expr) => {{
        let mut it = $iter;
        let mut out = Vec::new();
        for _ in 0..$n {
            match it.next().await? {
                Some(item) => out.push(item),
                None => break,
            }
        }
        out
    }};
}

/// Resolve a user-supplied chat reference to a `PeerRef`.
///
/// - `"me"` → the logged-in user (ambient authority, no network)
/// - `"@name"` / `"name"` → username resolution via the session
/// - numeric (Bot API dialog id) → `PeerId` + ambient authority
pub(crate) async fn resolve_chat_ref(client: &Client, chat: &str) -> Result<PeerRef> {
    let chat = chat.trim();
    if chat.eq_ignore_ascii_case("me") {
        return Ok(PeerId::self_user().to_ambient_ref());
    }
    if let Some(name) = chat.strip_prefix('@') {
        return resolve_username(client, name).await;
    }
    if chat.parse::<i64>().is_ok() {
        let id = parse_bot_api_chat_id(chat)?;
        return PeerId::from_bot_api_dialog_id(id)
            .map(|pid| pid.to_ambient_ref())
            .with_context(|| format!("chat id {chat:?} is not a valid dialog id"));
    }
    resolve_username(client, chat).await
}

async fn resolve_username(client: &Client, name: &str) -> Result<PeerRef> {
    let peer = client
        .resolve_username(name)
        .await
        .with_context(|| format!("username resolution failed for {name:?}"))?
        .with_context(|| format!("no chat found for username {name:?}"))?;
    peer.to_ref()
        .await
        .map_err(|e| anyhow::anyhow!("peer authority lookup failed for {name:?}: {e}"))?
        .with_context(|| format!("session has no authority for resolved peer {name:?}"))
}

/// Shared read path: `SearchIter` with empty query is the "browse"
/// mode (dates, no text filter), so every message read goes through
/// one code path. Fetches `n` items; caller decides paging.
async fn search_page(
    client: &Client,
    peer: PeerRef,
    query: &str,
    min: Option<DateTime<FixedOffset>>,
    max: Option<DateTime<FixedOffset>>,
    n: usize,
) -> Result<Vec<Message>> {
    let mut it = client.search_messages(peer).query(query);
    if let Some(d) = min {
        it = it.min_date(&d);
    }
    if let Some(d) = max {
        it = it.max_date(&d);
    }
    Ok(drain!(it, n))
}

pub(crate) async fn read_chat(client: &Client, cmd: &ReadChat) -> Result<Value> {
    // Thread reads ride the raw registry: grammers 0.10 builders do
    // not expose getReplies/getForumTopics, the typed TL layer does.
    if let Some(thread) = cmd.thread_id {
        return raw::run_raw(
            client,
            &Raw {
                method: "messages.getReplies".into(),
                params: json!({
                    "chat": cmd.chat,
                    "msg_id": thread,
                    "offset_id": 0,
                    "offset_date": 0,
                    "add_offset": 0,
                    "limit": cmd.limit.min(100),
                    "max_id": 0,
                    "min_id": 0,
                    "hash": 0
                }),
                confirm: true, // synthesized internally, post-authorize
            },
        )
        .await;
    }
    let peer = resolve_chat_ref(client, &cmd.chat).await?;
    let min = cmd.from.as_deref().map(parse_date).transpose()?;
    let max = cmd.to.as_deref().map(parse_date).transpose()?;
    let msgs = search_page(client, peer, "", min, max, cmd.limit as usize + 1).await?;
    let (messages, has_more) = truncate_with_more(
        msgs.iter().map(render::message).collect(),
        cmd.limit as usize,
    );
    Ok(json!({ "chat": cmd.chat, "messages": messages, "has_more": has_more }))
}

pub(crate) async fn search_chat(client: &Client, cmd: &SearchChat) -> Result<Value> {
    let peer = resolve_chat_ref(client, &cmd.chat).await?;
    let msgs = search_page(client, peer, &cmd.query, None, None, cmd.limit as usize + 1).await?;
    let (messages, has_more) = truncate_with_more(
        msgs.iter().map(render::message).collect(),
        cmd.limit as usize,
    );
    Ok(json!({ "chat": cmd.chat, "query": cmd.query, "messages": messages, "has_more": has_more }))
}

pub(crate) async fn search_global(client: &Client, cmd: &SearchGlobal) -> Result<Value> {
    let it = client.search_all_messages().query(&cmd.query);
    let msgs: Vec<Message> = drain!(it, cmd.limit as usize + 1);
    let (messages, has_more) = truncate_with_more(
        msgs.iter().map(render::message).collect(),
        cmd.limit as usize,
    );
    Ok(json!({ "query": cmd.query, "messages": messages, "has_more": has_more }))
}

pub(crate) async fn discover(client: &Client, cmd: &Discover) -> Result<Value> {
    // Phone discovery rides the raw registry (contacts.resolvePhone):
    // read-only, no contact import — unlike send_to_phone.
    if let Some(phone) = cmd.phone.as_deref() {
        let phone = normalize_phone(phone)?;
        return raw::run_raw(
            client,
            &Raw {
                method: "contacts.resolvePhone".into(),
                params: json!({ "phone": phone }),
                confirm: true, // synthesized internally, post-authorize
            },
        )
        .await;
    }
    if let Some(username) = cmd.username.as_deref() {
        let name = username.trim().trim_start_matches('@');
        let peer = client
            .resolve_username(name)
            .await
            .with_context(|| format!("username resolution failed for {name:?}"))?
            .with_context(|| format!("no chat found for username {name:?}"))?;
        return Ok(json!({ "chats": [render::peer(&peer)] }));
    }
    let dialogs = drain!(client.iter_dialogs(), cmd.limit as usize + 1);
    let (chats, has_more) = truncate_with_more(
        dialogs.iter().map(render::dialog).collect(),
        cmd.limit as usize,
    );
    Ok(json!({ "chats": chats, "has_more": has_more }))
}

/// Send a text message as the user. Irreversible: the message is
/// visible to the chat's members the moment this returns.
pub(crate) async fn send_text(client: &Client, cmd: &SendMessage) -> Result<Value> {
    let peer = resolve_chat_ref(client, &cmd.chat).await?;
    let mut message = InputMessage::new().text(cmd.text.clone());
    if let Some(reply_to) = cmd.reply_to {
        let id = narrow_message_id(reply_to)?;
        message = message.reply_to(Some(id));
    }
    let sent = client
        .send_message(peer, message)
        .await
        .map_err(|e| anyhow!("send to {} failed: {e}", cmd.chat))?;
    Ok(render::message(&sent))
}

/// Upload a local file and send it as a document (with optional
/// caption) as the user. Irreversible, and the upload itself is
/// visible in the chat even if the send later fails.
pub(crate) async fn send_document(client: &Client, cmd: &SendFile) -> Result<Value> {
    let path = std::path::Path::new(&cmd.path);
    if !path.is_file() {
        bail!("attachment {:?} is not a local file", cmd.path);
    }
    let peer = resolve_chat_ref(client, &cmd.chat).await?;
    let uploaded = client
        .upload_file(path)
        .await
        .map_err(|e| anyhow!("upload of {:?} failed: {e}", cmd.path))?;
    let caption = cmd.caption.clone().unwrap_or_default();
    let message = InputMessage::new().text(caption).document(uploaded);
    let sent = client
        .send_message(peer, message)
        .await
        .map_err(|e| anyhow!("send to {} failed: {e}", cmd.chat))?;
    Ok(render::message(&sent))
}

/// Edit a message the user previously sent. Irreversible: Telegram
/// keeps an edit trail; the original text is gone once this returns.
pub(crate) async fn edit_text(client: &Client, cmd: &EditMessage) -> Result<Value> {
    let id = narrow_message_id(cmd.message_id)?;
    let peer = resolve_chat_ref(client, &cmd.chat).await?;
    client
        .edit_message(peer, id, InputMessage::new().text(cmd.new_text.clone()))
        .await
        .map_err(|e| anyhow!("edit of {}#{} failed: {e}", cmd.chat, cmd.message_id))?;
    Ok(json!({
        "edited": true,
        "chat": cmd.chat,
        "message_id": cmd.message_id,
    }))
}

/// Send a text to a phone number: `contacts.importContacts` creates a
/// temporary contact under the user's account, then the text goes to
/// the imported peer. Irreversible twice over — the contact import is
/// visible in the account's contact list hygiene, and the message is
/// delivered as the user.
pub(crate) async fn send_phone(client: &Client, cmd: &SendToPhone) -> Result<Value> {
    let phone = normalize_phone(&cmd.phone)?;
    let contact = tl::types::InputPhoneContact {
        client_id: 0,
        phone: phone.clone(),
        first_name: "OpenCrabs contact".into(),
        last_name: String::new(),
        note: None,
    };
    let import = tl::functions::contacts::ImportContacts {
        contacts: vec![tl::enums::InputContact::InputPhoneContact(contact)],
    };
    let tl::enums::contacts::ImportedContacts::Contacts(result) = client
        .invoke(&import)
        .await
        .map_err(|e| anyhow!("importContacts failed for {phone}: {e}"))?;
    let user = result
        .users
        .into_iter()
        .next()
        .with_context(|| format!("phone {phone} did not resolve to a Telegram user"))?;
    let tl::enums::User::User(u) = user else {
        bail!("Telegram returned an empty user for {phone}");
    };
    let hash = u
        .access_hash
        .with_context(|| format!("no access hash for user {}", u.id))?;
    let id = PeerId::user(u.id).with_context(|| format!("invalid user id {}", u.id))?;
    let peer = PeerRef {
        id,
        auth: PeerAuth::from_hash(hash),
    };
    let sent = client
        .send_message(peer, InputMessage::new().text(cmd.text.clone()))
        .await
        .map_err(|e| anyhow!("send to {phone} failed: {e}"))?;
    Ok(render::message(&sent))
}
