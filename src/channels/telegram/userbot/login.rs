//! Userbot login flows: QR (default) and phone-code, both with cloud-password
//! (2FA SRP) completion. Ported from the spike that completed a real login on
//! 2026-08-19; the grammers 0.10 wiring is verified against vendored source
//! (docs lag two majors; trust this file and the compiler, not the docs).
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

use super::runner::AbortOnDrop;
use super::session::FileSession;
use super::{UserbotCreds, resolve_creds, session_file};
use crate::config::types::{Config, TelegramUserbotConfig};

/// Connect a client on the configured session, driving I/O on a background task.
/// Returns the client, a session handle (for post-auth persistence), the
/// update receiver to hand to the watch loop, and the guard that owns the
/// connection driver: drop it and the sockets close.
pub(crate) async fn connect(
    cfg: &TelegramUserbotConfig,
) -> Result<(
    Client,
    Arc<FileSession>,
    mpsc::UnboundedReceiver<grammers_session::updates::UpdatesLike>,
    AbortOnDrop,
)> {
    let creds = resolve_creds(cfg)?;
    let path = session_file(cfg);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating session directory {}", parent.display()))?;
    }
    let session = Arc::new(FileSession::load(&path)?);
    // The session file IS the logged-in account, same belt keys.toml wears.
    // A failed chmod is not fatal (the file may not exist yet, or the
    // filesystem may not carry modes) but it must never be silent: an
    // account session left world-readable is exactly the thing to log.
    #[cfg(unix)]
    if path.is_file() {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        {
            tracing::warn!(
                path = %path.display(),
                "Telegram userbot session file could not be set to 0600: {error}"
            );
        }
    }
    let SenderPool {
        runner,
        handle,
        updates,
    } = SenderPool::new(session.clone(), creds.api_id);
    let client = Client::new(handle);
    let runner = AbortOnDrop::new(tokio::spawn(runner.run()));
    Ok((client, session, updates, runner))
}

async fn prompt_password(line: &'static str) -> Result<String> {
    tokio::task::spawn_blocking(move || rpassword::prompt_password(line))
        .await
        .context("joining password prompt")?
        .context("reading hidden password")
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

/// Cloud-password (SRP) completion: raw-TL replication of grammers' private
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
        .ok_or_else(|| anyhow::anyhow!("no current_algo; is a cloud password actually set?"))?;
    let tl::types::PasswordKdfAlgoSha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow {
        salt1,
        salt2,
        p,
        g,
    } = match algo {
        tl::enums::PasswordKdfAlgo::Sha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow(a) => a,
        tl::enums::PasswordKdfAlgo::Unknown => {
            anyhow::bail!("unknown KDF algorithm; client outdated?")
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

/// Post-authorization persistence: replicates grammers' private complete_login.
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
    // Seeding the update state is best effort: without it the watch loop
    // starts from Telegram's current state on first connect, which loses
    // nothing for receive-only capture. It still has to say when it fails.
    match client.invoke(&tl::functions::updates::GetState {}).await {
        Ok(tl::enums::updates::State::State(s)) => {
            if let Err(error) = session
                .set_update_state(UpdateState::All(UpdatesState {
                    pts: s.pts,
                    qts: s.qts,
                    date: s.date,
                    seq: s.seq,
                    channels: Vec::new(),
                }))
                .await
            {
                tracing::warn!("Telegram userbot could not seed update state: {error}");
            }
        }
        Err(error) => {
            tracing::warn!("Telegram userbot updates.getState failed after login: {error}");
        }
    }
    Ok(u.first_name.clone().unwrap_or_default())
}

/// One QR polling round-trip. Handles export, DC migration/import, and
/// password-needed detection for the terminal login flow.
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
/// any chat, so it is immune to the anti-phishing tripwire that invalidates pasted
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
                let pass = prompt_password("2FA cloud password: ").await?;
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
/// typed here in the terminal, since codes pasted into any Telegram chat are
/// invalidated by Telegram's anti-phishing tripwire.
pub(crate) async fn code_login(client: Client, creds: &UserbotCreds) -> Result<String> {
    let token = client
        .request_login_code(&creds.phone, &creds.api_hash)
        .await
        .context("requesting login code")?;
    let code = prompt_password("Login code (sent to Telegram): ").await?;
    match client.sign_in(&token, &code).await {
        Ok(u) => Ok(u.first_name().unwrap_or_default().to_string()),
        Err(SignInError::PasswordRequired(pw)) => {
            let pass = prompt_password("2FA cloud password: ").await?;
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

    let (client, session, updates, runner) = connect(cfg).await?;
    // The CLI never consumes updates; the receiver only has to outlive the
    // pool so the runner does not see a closed channel mid-login.
    if client.is_authorized().await? {
        let me = client.get_me().await?;
        println!(
            "✅ already authorized as {}",
            me.first_name().unwrap_or("?")
        );
        drop(updates);
        drop(runner);
        return Ok(());
    }
    let name = if use_code {
        code_login(client.clone(), &creds).await?
    } else {
        qr_login(client.clone(), session.clone(), &creds).await?
    };
    // finish() only mutated in-memory state; the CLI exits here, so persist
    // the freshly-earned session NOW or it is lost.
    session.save()?;
    drop(updates);
    drop(runner);
    println!("✅ authorized as {name}");
    println!(
        "The session file grants full account access. Treat it like keys.toml.\n\
         Restart opencrabs (or toggle channels.telegram.userbot.enabled) to start the watch loop."
    );
    Ok(())
}

/// True when the local session file exists. Authorization is verified by the
/// watch loop before it starts consuming updates.
pub(crate) fn session_exists(config: &TelegramUserbotConfig) -> bool {
    session_file(config).is_file()
}
