//! Pre-swap probes of a downloaded binary: does it start and report its
//! version, and how many migrations does it carry (so an older release is
//! never swapped over a database it cannot open).

/// Run a health check on a binary: execute it with `--version`,
/// verify it exits cleanly within a timeout. Returns a detailed error
/// with stderr output on failure.
pub(super) async fn health_check_binary(path: &std::path::Path) -> std::result::Result<(), String> {
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    tracing::info!(
        target: "evolve",
        path = %path.display(),
        size = file_size,
        "evolve: running `<binary> --version` health check"
    );

    // A binary written moments ago can briefly fail to exec even though it is
    // perfectly valid: ETXTBSY (os 26 — kernel still treats it as open for
    // write) or a transient ENOENT (os 2) before the write fully settles. This
    // broke evolve on a VPS — one run failed to spawn with os 2, the next with
    // os 26, both on the same 79MB binary. Retry the spawn a few times with
    // backoff before treating it as a real failure.
    let mut attempt = 0u32;
    let result = loop {
        attempt += 1;
        let r = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio::process::Command::new(path)
                .kill_on_drop(true)
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output(),
        )
        .await;

        if let Ok(Err(ref e)) = r
            && attempt < 5
            && matches!(e.raw_os_error(), Some(2) | Some(26))
        {
            tracing::warn!(
                target: "evolve",
                attempt,
                os_error = ?e.raw_os_error(),
                "evolve: fresh binary not exec-able yet, retrying health check"
            );
            tokio::time::sleep(std::time::Duration::from_millis(200 * attempt as u64)).await;
            continue;
        }
        break r;
    };

    match result {
        Ok(Ok(output)) if output.status.success() => Ok(()),
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr_snippet: String = stderr.chars().take(200).collect();
            tracing::warn!(
                target: "evolve",
                path = %path.display(),
                exit_status = %output.status,
                size = file_size,
                stderr_excerpt = %stderr_snippet,
                "evolve: health check exited non-zero"
            );
            Err(format!(
                "exited with {} (binary: {} bytes, platform: {}/{}{})",
                output.status,
                file_size,
                std::env::consts::OS,
                std::env::consts::ARCH,
                if stderr_snippet.is_empty() {
                    String::new()
                } else {
                    format!(", stderr: {}", stderr_snippet)
                }
            ))
        }
        Ok(Err(e)) => {
            tracing::warn!(
                target: "evolve",
                path = %path.display(),
                error = %e,
                size = file_size,
                "evolve: health check failed to spawn the binary"
            );
            Err(format!(
                "failed to spawn: {e} (binary: {file_size} bytes, platform: {}/{})",
                std::env::consts::OS,
                std::env::consts::ARCH
            ))
        }
        Err(_) => {
            tracing::warn!(
                target: "evolve",
                path = %path.display(),
                size = file_size,
                "evolve: health check timed out after 10s"
            );
            Err(format!("timed out after 10s (binary: {file_size} bytes)"))
        }
    }
}

/// Run the binary with `print-migration-count` and return the parsed count.
/// Returns an error if the binary doesn't support this flag or the output
/// can't be parsed.
pub(super) async fn get_binary_migration_count(
    path: &std::path::Path,
) -> std::result::Result<usize, String> {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(path)
            .kill_on_drop(true)
            .arg("print-migration-count")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let count = stdout
                .trim()
                .parse::<usize>()
                .map_err(|e| format!("could not parse migration count from '{stdout}': {e}"))?;
            Ok(count)
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let snippet: String = stderr.chars().take(200).collect();
            Err(format!(
                "print-migration-count exited with {}: {}",
                output.status,
                if snippet.is_empty() {
                    "no stderr"
                } else {
                    &snippet
                }
            ))
        }
        Ok(Err(e)) => Err(format!("failed to spawn binary: {e}")),
        Err(_) => Err("timed out after 10s".into()),
    }
}
