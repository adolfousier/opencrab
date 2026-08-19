//! Userbot login flows: QR (default) and phone-code, both with cloud-password
//! (2FA SRP) completion. Ported from the spike that completed a real login on
//! 2026-08-19; the grammers 0.10 wiring is verified against vendored source
//! (docs lag two majors — trust this file and the compiler, not the docs).
//!
//! Grammers 0.10 construction (differs from every published example):
//!   FileSession::load(path) -> SenderPool::new(session, api_id) ->
//!   destructure {runner, handle, updates} -> Client::new(handle) ->
//!   spawn runner.run() to drive connections on demand.
//!
//! The post-auth persistence replicates grammers' private `complete_login`
//! with public Session API only: cache the self peer (with auth hash) and
//! seed the update state, so the session is immediately usable by the watch
//! loop without re-auth.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use grammers_client::{Client, SignInError, tl};
use grammers_mtsender::SenderPool;
use grammers_session::Session as _;
use grammers_session::types::{PeerAuth, PeerInfo, UpdateState, UpdatesState};
use tokio::sync::mpsc;

use super::session::FileSession;
use super::{UserbotCreds, resolve_creds, session_file};
use crate::config::types::{Config, TelegramUserbotConfig};

/// Connect a client on the configured session, driving I/O on a background task.
/// Returns the client, a session handle (for post-auth persistence), and the
/// update receiver to hand to the watch loop.
pub(crate) async fn connect(
    cfg: &TelegramUserbotConfig,
) -> Result<(
    Client,
    Arc<FileSession>,
    mpsc::UnboundedReceiver<grammers_session::updates::UpdatesLike>,
)> {
    let creds = resolve_creds(cfg)?;
    let path = session_file(cfg);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let session = Arc::new(FileSession::load(&path)?);
    // The session file IS the logged-in account — same belt keys.toml wears.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    let SenderPool {
        runner,
        handle,
        updates,
    } = SenderPool::new(session.clone(), creds.api_id);
    let client = Client::new(handle);
    tokio::spawn(runner.run());
    Ok((client, session, updates))
}

async fn prompt(line: &str) -> Result<String> {
    use tokio::io::{AsyncBufReadExt as _, BufReader};
    println!("{line}");
    let mut buf = String::new();
    BufReader::new(tokio::io::stdin())
        .read_line(&mut buf)
        .await
        .context("reading stdin")?;
    Ok(buf.trim().to_owned())
}

/// Render `tg://login?token=…` as a scannable QR directly in the terminal.
fn render_qr_terminal(token: &[u8]) -> Result<()> {
    let url = format!("tg://login?token={}", URL_SAFE_NO_PAD.encode(token));
    let code = qrcode::QrCode::new(url.as_bytes()).context("building QR")?;
    let art = code
        .render::<char>()
        .quiet_zone(true)
        .module_dimensions(2, 1)
        .build();
    // Blank lines keep the QR clear of shell prompt debris above it.
    println!("\n\n{art}\n");
    Ok(())
}

/// Cloud-password (SRP) completion — raw-TL replication of grammers' private
/// `check_password`: account.GetPassword -> calculate_2fa -> auth.CheckPassword.
/// `pass` is collected by the caller, never echoed into logs.
pub(crate) async fn password_step(
    client: &Client,
    pass: String,
) -> Result<tl::enums::auth::Authorization> {
    let tl::enums::account::Password::Password(pw) = client
        .invoke(&tl::functions::account::GetPassword {})
        .await
        .context("account.GetPassword")?;
    let algo = pw
        .current_algo
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no current_algo — is a cloud password actually set?"))?;
    let tl::types::PasswordKdfAlgoSha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow {
        salt1,
        salt2,
        p,
        g,
    } = match algo {
        tl::enums::PasswordKdfAlgo::Sha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow(a) => a,
        tl::enums::PasswordKdfAlgo::Unknown => {
            anyhow::bail!("unknown KDF algorithm — client outdated?")
        }
    };
    let (m1, g_a) = grammers_crypto::two_factor_auth::calculate_2fa(
        salt1,
        salt2,
        p,
        g,
        pw.srp_b
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no srp_b"))?,
        pw.secure_random.clone(),
        pass,
    );
    client
        .invoke(&tl::functions::auth::CheckPassword {
            password: tl::enums::InputCheckPasswordSrp::Srp(tl::types::InputCheckPasswordSrp {
                srp_id: pw.srp_id.ok_or_else(|| anyhow::anyhow!("no srp_id"))?,
                a: g_a.to_vec(),
                m1: m1.to_vec(),
            }),
        })
        .await
        .map_err(|e| anyhow::anyhow!("2FA check failed (wrong password?): {e}"))
}

/// Post-authorization persistence — replicates grammers' private complete_login.
pub(crate) async fn finish(
    client: &Client,
    session: &Arc<FileSession>,
    authorization: tl::enums::auth::Authorization,
) -> Result<String> {
    let tl::enums::auth::Authorization::Authorization(auth) = authorization else {
        anyhow::bail!("unexpected authorization variant");
    };
    let tl::enums::User::User(u) = auth.user else {
        anyhow::bail!("empty user in authorization");
    };
    let hash = u
        .access_hash
        .ok_or_else(|| anyhow::anyhow!("no access_hash on self"))?;
    session
        .cache_peer(&PeerInfo::User {
            id: u.id,
            auth: Some(PeerAuth::from_hash(hash)),
            bot: Some(u.bot),
            is_self: Some(true),
        })
        .await
        .context("caching self peer")?;
    if let Ok(tl::enums::updates::State::State(s)) =
        client.invoke(&tl::functions::updates::GetState {}).await
    {
        let _ = session
            .set_update_state(UpdateState::All(UpdatesState {
                pts: s.pts,
                qts: s.qts,
                date: s.date,
                seq: s.seq,
                channels: Vec::new(),
            }))
            .await;
    }
    Ok(u.first_name.clone().unwrap_or_default())
}

/// One QR polling round-trip. Owns the subtle MTProto parts — export, DC
/// migration + import, password-needed sniffing — exactly once, shared by the
/// terminal CLI flow and the chat-driven flow in [`super::chat_login`].
pub(crate) enum QrStep {
    /// Scanned and authorized (directly or after DC migration).
    Success(Box<tl::enums::auth::Authorization>),
    /// The account has a cloud password: the caller must collect it and run
    /// [`password_step`].
    PasswordNeeded,
    /// Current token bytes; re-render only when they changed.
    Token(Vec<u8>),
}

pub(crate) async fn qr_poll_once(client: &Client, creds: &UserbotCreds) -> Result<QrStep> {
    let res = match client
        .invoke(&tl::functions::auth::ExportLoginToken {
            api_id: creds.api_id,
            api_hash: creds.api_hash.clone(),
            except_ids: Vec::new(),
        })
        .await
    {
        Ok(res) => res,
        Err(e) if e.is("SESSION_PASSWORD_NEEDED") => return Ok(QrStep::PasswordNeeded),
        Err(e) => return Err(e.into()),
    };
    match res {
        tl::enums::auth::LoginToken::Success(s) => Ok(QrStep::Success(Box::new(s.authorization))),
        tl::enums::auth::LoginToken::Token(t) => Ok(QrStep::Token(t.token)),
        tl::enums::auth::LoginToken::MigrateTo(m) => {
            println!("migrating to DC {}…", m.dc_id);
            match client
                .invoke_in_dc(
                    m.dc_id,
                    &tl::functions::auth::ImportLoginToken { token: m.token },
                )
                .await
            {
                Ok(tl::enums::auth::LoginToken::Success(s)) => {
                    Ok(QrStep::Success(Box::new(s.authorization)))
                }
                Ok(other) => anyhow::bail!("import after migrate: unexpected {other:?}"),
                Err(e) if e.is("SESSION_PASSWORD_NEEDED") => Ok(QrStep::PasswordNeeded),
                Err(e) => Err(e.into()),
            }
        }
    }
}

/// QR login (terminal flavor): render the token in the terminal, poll until
/// scanned; on SESSION_PASSWORD_NEEDED finish via SRP. No codes, nothing in
/// any chat — immune to the anti-phishing tripwire that invalidates pasted
/// codes.
pub(crate) async fn qr_login(
    client: Client,
    session: Arc<FileSession>,
    creds: &UserbotCreds,
) -> Result<String> {
    let mut last: Vec<u8> = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    loop {
        anyhow::ensure!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for QR scan"
        );
        match qr_poll_once(&client, creds).await? {
            QrStep::Success(auth) => return finish(&client, &session, *auth).await,
            QrStep::PasswordNeeded => {
                let pass = prompt("2FA password (cloud password) — input is NOT hidden:").await?;
                let auth = password_step(&client, pass).await?;
                return finish(&client, &session, auth).await;
            }
            QrStep::Token(t) => {
                if t != last {
                    last = t.clone();
                    render_qr_terminal(&t)?;
                    println!(
                        "Scan with your phone: Telegram > Settings > Devices > Link Desktop Device\n(valid ~3 min; re-renders automatically)"
                    );
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// Phone-code login (fallback when the camera isn't available). The code is
/// typed here in the terminal — codes pasted into any Telegram chat are
/// invalidated by Telegram's anti-phishing tripwire.
pub(crate) async fn code_login(
    client: Client,
    _session: Arc<FileSession>,
    creds: &UserbotCreds,
) -> Result<String> {
    let token = client
        .request_login_code(&creds.phone, &creds.api_hash)
        .await
        .context("requesting login code")?;
    let code = prompt("Login code (sent to your Telegram):").await?;
    match client.sign_in(&token, &code).await {
        Ok(u) => Ok(u.first_name().unwrap_or_default().to_string()),
        Err(SignInError::PasswordRequired(pw)) => {
            let pass = prompt("2FA password (cloud password) — input is NOT hidden:").await?;
            let user = client.check_password(pw, pass).await?;
            Ok(user.first_name().unwrap_or_default().to_string())
        }
        Err(e) => Err(e.into()),
    }
}

/// `opencrabs channel userbot-login [--code]` entry point.
pub(crate) async fn cmd_userbot_login(config: &Config, use_code: bool) -> Result<()> {
    let cfg = &config.channels.telegram.userbot;
    let creds = resolve_creds(cfg)?;
    let path = session_file(cfg);
    println!("userbot session file: {}", path.display());

    let (client, session, _updates) = connect(cfg).await?;
    if client.is_authorized().await? {
        let me = client.get_me().await?;
        println!(
            "✅ already authorized as {}",
            me.first_name().unwrap_or("?")
        );
        return Ok(());
    }
    let name = if use_code {
        code_login(client.clone(), session.clone(), &creds).await?
    } else {
        qr_login(client.clone(), session.clone(), &creds).await?
    };
    // finish() only mutated in-memory state; the CLI exits here, so persist
    // the freshly-earned session NOW or it is lost.
    session.save()?;
    println!("✅ authorized as {name}");
    println!(
        "The session file grants full account access — treat it like keys.toml.\n\
         Restart opencrabs (or toggle channels.telegram.userbot.enabled) to start the watch loop."
    );
    Ok(())
}

/// Config gate helper for the watch loop: is the userbot enabled AND does a
/// session already exist? A missing session is a configuration problem, not a
/// crash: the caller logs the login instruction instead.
pub(crate) fn session_exists(cfg: &TelegramUserbotConfig) -> bool {
    session_file(cfg).is_file()
}
