//! The two ends of the drop tunnel (#1289).
//!
//! [`serve`] runs on the machine holding the file, behind an `ssh -R` forward.
//! [`fetch`] runs wherever the TUI is and dials the forwarded port.
//!
//! The wire format is one request line (an absolute path), one response line,
//! then the bytes. Small on purpose: the security work is in
//! [`super::drop_transfer::authorize`], not in the framing.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use super::drop_transfer::{MAX_TRANSFER_BYTES, Response, authorize, default_roots};

/// How long the requesting side waits. A tunnel that is up answers at once;
/// one that is not should fail fast so the user gets the copy hint instead of
/// a hang.
const DIAL_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// Serve dropped files to the far end of the tunnel, forever.
///
/// Binds loopback ONLY. The forward is what makes it reachable from the
/// server, so binding any interface would expose the client's files to its
/// own network for no benefit.
pub fn serve(port: u16, roots: Vec<PathBuf>) -> Result<()> {
    let roots = if roots.is_empty() {
        default_roots()
    } else {
        roots
    };
    if roots.is_empty() {
        bail!("no servable directories resolved; pass --root explicitly");
    }

    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = TcpListener::bind(addr)
        .with_context(|| format!("cannot bind {addr}; is another agent already running?"))?;

    println!("OpenCrabs drop agent listening on {addr}");
    println!("Serving files from:");
    for root in &roots {
        println!("  {}", root.display());
    }
    println!(
        "\nOn the machine running the TUI, connect with:\n  \
         ssh -R {port}:localhost:{port} <you>@<host>\n"
    );

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                // Serial by design. A drop is one file at a time, and serving
                // the client's filesystem is not something to widen casually.
                if let Err(e) = handle(stream, &roots) {
                    tracing::warn!("drop agent: request failed: {e:#}");
                }
            }
            Err(e) => tracing::warn!("drop agent: accept failed: {e}"),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, roots: &[PathBuf]) -> Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_write_timeout(Some(READ_TIMEOUT))?;

    let mut requested = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut requested)?;
    let requested = requested.trim();
    if requested.is_empty() {
        return Ok(());
    }

    match authorize(requested, roots) {
        Ok(path) => {
            let bytes = std::fs::read(&path)?;
            // Every served path is logged. The whole point of the guard is
            // that this list is reviewable.
            println!(
                "drop agent: serving {} ({} bytes)",
                path.display(),
                bytes.len()
            );
            stream.write_all(
                Response::Ok {
                    len: bytes.len() as u64,
                }
                .encode()
                .as_bytes(),
            )?;
            stream.write_all(&bytes)?;
        }
        Err(refusal) => {
            // Refusals are announced on the agent's own stdout too: a server
            // probing for files the user never dropped should be visible on
            // the machine being probed, not only to the machine probing.
            println!("drop agent: REFUSED {requested} ({})", refusal.reason());
            stream.write_all(
                Response::Err {
                    reason: refusal.reason(),
                }
                .encode()
                .as_bytes(),
            )?;
        }
    }
    stream.flush()?;
    Ok(())
}

/// Ask the agent for `client_path` and return its bytes.
///
/// Runs on the machine with the TUI. Every failure is an ordinary outcome
/// here: no tunnel, no agent, or a refusal are all things the caller reports
/// and then falls back from.
pub fn fetch(port: u16, client_path: &str) -> Result<Vec<u8>> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, DIAL_TIMEOUT)
        .with_context(|| format!("no drop agent answering on {addr}"))?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_write_timeout(Some(READ_TIMEOUT))?;

    writeln!(stream, "{client_path}")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let Some(response) = Response::parse(&line) else {
        bail!("drop agent sent an unreadable response: {line:?}");
    };

    match response {
        Response::Err { reason } => bail!("drop agent refused: {reason}"),
        Response::Ok { len } => {
            if len > MAX_TRANSFER_BYTES {
                bail!("drop agent offered {len} bytes, over the transfer limit");
            }
            let mut bytes = vec![0u8; len as usize];
            reader
                .read_exact(&mut bytes)
                .with_context(|| format!("transfer cut short before {len} bytes"))?;
            Ok(bytes)
        }
    }
}
