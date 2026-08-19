//! Conversational setup for the userbot — the deterministic boundary.
//!
//! Chat-driven login needs exactly three verifiable behaviors: recognizing
//! the command, validating credentials by shape (naming the bad field), and
//! writing them to keys.toml without touching anything else in the file.
//! Everything network-shaped lives in [`super::login`] / [`super::chat_login`].
//!
//! `api_hash` is a secret: it is never echoed in errors, replies, or logs —
//! validation messages name fields and shapes, never values.

use crate::config::{atomic_write, keys_path};

/// Fully validated MTProto app credentials (my.telegram.org).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CollectedCredentials {
    pub api_id: i64,
    pub api_hash: String,
    pub phone: String,
}

/// Recognize `/userbot-login` / `/userbot_login` (optionally `@botname`),
/// returning the trailing argument string (`""` when bare).
///
/// `/userbot-login-now` and friends are NOT the command: exact word match on
/// the hyphen/underscore forms only.
pub(crate) fn parse_login_command(text: &str) -> Option<&str> {
    let text = text.trim();
    let rest = text.strip_prefix('/')?;
    let word = rest.split_whitespace().next()?;
    let bare = word.split('@').next().unwrap_or(word);
    if bare != "userbot-login" && bare != "userbot_login" {
        return None;
    }
    Some(text[1 + word.len()..].trim_start())
}

/// Shape validators — errors name the field so the owner can fix exactly one
/// value without re-sending the rest.
fn valid_api_id(s: &str) -> Result<i64, String> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(
            "api_id must be pure digits (from my.telegram.org → API development tools)".into(),
        );
    }
    let v: i64 = s
        .parse()
        .map_err(|_| "api_id is not a valid number".to_string())?;
    if v > i32::MAX as i64 {
        return Err("api_id is out of range".into());
    }
    Ok(v)
}

fn valid_api_hash(s: &str) -> Result<String, String> {
    if s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(s.to_owned())
    } else {
        Err("api_hash must be exactly 32 hexadecimal characters".into())
    }
}

fn valid_phone(s: &str) -> Result<String, String> {
    let ok = s
        .strip_prefix('+')
        .is_some_and(|d| (10..=15).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()));
    if ok {
        Ok(s.to_owned())
    } else {
        Err("phone must start with '+' followed by 10-15 digits (e.g. +2547…)".into())
    }
}

/// Partially-collected credentials. Values arrive either as one positional
/// triple (`/userbot-login <id> <hash> <phone>`) or unordered across several
/// messages — the draft classifies each value by its shape.
#[derive(Debug, Default, Clone)]
pub(crate) struct CredentialDraft {
    api_id: Option<i64>,
    api_hash: Option<String>,
    phone: Option<String>,
}

impl CredentialDraft {
    /// Ingest `<api_id> <api_hash> <phone>` in order. All-or-nothing: a bad
    /// value rejects the whole triple, keeping slots consistent.
    pub(crate) fn ingest_positional(&mut self, args: &str) -> Result<(), String> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(format!(
                "expected exactly 3 values: <api_id> <api_hash> <phone> (got {})",
                parts.len()
            ));
        }
        let api_id = valid_api_id(parts[0])?;
        let api_hash = valid_api_hash(parts[1])?;
        let phone = valid_phone(parts[2])?;
        self.api_id = Some(api_id);
        self.api_hash = Some(api_hash);
        self.phone = Some(phone);
        Ok(())
    }

    /// Ingest whitespace-separated values classified by shape, any order:
    /// 32 hex chars → api_hash, `+digits` → phone, digits → api_id.
    /// Hash is classified before id so a 32-char token never lands in api_id.
    pub(crate) fn ingest_unordered(&mut self, values: &str) -> Result<(), String> {
        for token in values.split_whitespace() {
            self.ingest_one(token)?;
        }
        Ok(())
    }

    fn ingest_one(&mut self, token: &str) -> Result<(), String> {
        let dup = |field: &str| {
            format!("{field} already set — send 'cancel' or /userbot-login to restart")
        };
        if token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()) {
            if self.api_hash.is_some() {
                return Err(dup("api_hash"));
            }
            self.api_hash = Some(valid_api_hash(token)?);
            return Ok(());
        }
        if valid_phone(token).is_ok() {
            if self.phone.is_some() {
                return Err(dup("phone"));
            }
            self.phone = Some(token.to_owned());
            return Ok(());
        }
        if !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit()) {
            if self.api_id.is_some() {
                return Err(dup("api_id"));
            }
            self.api_id = Some(valid_api_id(token)?);
            return Ok(());
        }
        Err(
            "couldn't classify that value — expected api_id (digits), api_hash (32 hex chars), \
             or phone (+…10-15 digits)"
                .to_string(),
        )
    }

    /// Unset field names in canonical order (api_id, api_hash, phone).
    pub(crate) fn missing_names(&self) -> Vec<&'static str> {
        [
            self.api_id.is_none().then_some("api_id"),
            self.api_hash.is_none().then_some("api_hash"),
            self.phone.is_none().then_some("phone"),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.api_id.is_some() && self.api_hash.is_some() && self.phone.is_some()
    }

    pub(crate) fn complete(self) -> Result<CollectedCredentials, String> {
        if let (Some(api_id), Some(api_hash), Some(phone)) =
            (&self.api_id, &self.api_hash, &self.phone)
        {
            return Ok(CollectedCredentials {
                api_id: *api_id,
                api_hash: api_hash.clone(),
                phone: phone.clone(),
            });
        }
        Err(format!(
            "still missing: {}",
            self.missing_names().join(", ")
        ))
    }
}

/// Persist credentials into `[channels.telegram.userbot]` in keys.toml.
///
/// Format-preserving merge (toml_edit): every other key, table, and comment
/// in the file survives untouched. Atomic write + 0600 — this file already
/// holds every secret the bot has.
pub(crate) fn persist_credentials(creds: &CollectedCredentials) -> anyhow::Result<()> {
    use toml_edit::DocumentMut;

    let path = keys_path();
    let mut doc: DocumentMut = if path.exists() {
        std::fs::read_to_string(&path)?.parse()?
    } else {
        DocumentMut::new()
    };

    let mut table = doc.as_table_mut();
    for part in ["channels", "telegram", "userbot"] {
        if table.get(part).is_none() {
            table.insert(part, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        table = table
            .get_mut(part)
            .and_then(|item| item.as_table_mut())
            .ok_or_else(|| anyhow::anyhow!("keys.toml: '{part}' exists but is not a table"))?;
    }
    table.insert("api_id", toml_edit::value(creds.api_id));
    table.insert("api_hash", toml_edit::value(creds.api_hash.as_str()));
    table.insert("phone", toml_edit::value(creds.phone.as_str()));

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic_write(&path, &doc.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    tracing::info!("userbot: credentials persisted to keys.toml [channels.telegram.userbot]");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validators_name_their_field() {
        assert!(
            valid_api_hash("nothex")
                .unwrap_err()
                .contains("32 hexadecimal")
        );
        assert!(valid_phone("254769000111").unwrap_err().contains("phone"));
        assert_eq!(valid_api_id("25625345").unwrap(), 25_625_345);
    }

    #[test]
    fn positional_rejects_bad_values_without_partial_writes() {
        let mut draft = CredentialDraft::default();
        let err = draft
            .ingest_positional("1 0123456789abcdef0123456789abcdef +254769000111 bad")
            .unwrap_err();
        assert!(err.contains("3 values"));
        assert!(!draft.is_complete(), "failed ingest must not fill slots");
    }
}
