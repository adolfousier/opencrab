//! Secure secret management
//!
//! This module provides secure handling of sensitive data like API keys,
//! ensuring they are properly zeroized from memory when dropped.
//! Supports OS keyring integration for secure persistent storage.

use anyhow::{Context, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Service name for keyring entries
const KEYRING_SERVICE: &str = "opencrabs";

/// A secure string that zeroizes its contents on drop
///
/// This type should be used for any sensitive data like API keys,
/// passwords, or tokens to ensure they are properly cleared from memory.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretString {
    inner: String,
}

impl SecretString {
    /// Create a new SecretString from a String
    pub fn new(value: String) -> Self {
        Self { inner: value }
    }

    /// Create a new SecretString from a &str
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Self {
        Self {
            inner: value.to_string(),
        }
    }

    /// Load a secret from an environment variable
    pub fn from_env(var_name: &str) -> Result<Self> {
        let value = std::env::var(var_name)
            .with_context(|| format!("Environment variable not found: {}", var_name))?;
        Ok(Self::new(value))
    }

    /// Load an optional secret from an environment variable
    pub fn from_env_optional(var_name: &str) -> Option<Self> {
        std::env::var(var_name).ok().map(Self::new)
    }

    /// Load a secret from OS keyring
    ///
    /// This provides secure persistent storage for API keys using the operating system's
    /// credential manager (Windows Credential Manager, macOS Keychain, Linux Secret Service).
    pub fn from_keyring(key_name: &str) -> Result<Self> {
        let entry = Entry::new(KEYRING_SERVICE, key_name)
            .with_context(|| format!("Failed to access keyring for {}", key_name))?;

        let password = entry
            .get_password()
            .with_context(|| format!("Failed to retrieve secret from keyring: {}", key_name))?;

        Ok(Self::new(password))
    }

    /// Load an optional secret from OS keyring
    pub fn from_keyring_optional(key_name: &str) -> Option<Self> {
        Entry::new(KEYRING_SERVICE, key_name)
            .ok()
            .and_then(|entry| entry.get_password().ok())
            .map(Self::new)
    }

    /// Save this secret to OS keyring
    ///
    /// # Security
    /// This stores the API key securely in the operating system's credential storage,
    /// which is encrypted and protected by the OS.
    pub fn save_to_keyring(&self, key_name: &str) -> Result<()> {
        let entry = Entry::new(KEYRING_SERVICE, key_name)
            .with_context(|| format!("Failed to access keyring for {}", key_name))?;

        entry
            .set_password(self.expose_secret())
            .with_context(|| format!("Failed to save secret to keyring: {}", key_name))?;

        tracing::info!("Saved secret '{}' to OS keyring", key_name);
        Ok(())
    }

    /// Delete this secret from OS keyring
    pub fn delete_from_keyring(key_name: &str) -> Result<()> {
        let entry = Entry::new(KEYRING_SERVICE, key_name)
            .with_context(|| format!("Failed to access keyring for {}", key_name))?;

        entry
            .delete_credential()
            .with_context(|| format!("Failed to delete secret from keyring: {}", key_name))?;

        tracing::info!("Deleted secret '{}' from OS keyring", key_name);
        Ok(())
    }

    /// Load secret with fallback priority: keyring → env → none
    ///
    /// This is the recommended way to load API keys. It tries:
    /// 1. OS keyring (most secure, persistent)
    /// 2. Environment variable (temporary, less secure)
    /// 3. None (not configured)
    pub fn load_with_fallback(key_name: &str, env_var: &str) -> Option<Self> {
        // Try keyring first (most secure)
        if let Some(secret) = Self::from_keyring_optional(key_name) {
            tracing::debug!("Loaded '{}' from OS keyring", key_name);
            return Some(secret);
        }

        // Fall back to environment variable
        if let Some(secret) = Self::from_env_optional(env_var) {
            tracing::debug!(
                "Loaded '{}' from environment variable {}",
                key_name,
                env_var
            );
            return Some(secret);
        }

        tracing::debug!(
            "No secret found for '{}' in keyring or environment",
            key_name
        );
        None
    }

    /// Get a reference to the inner string
    ///
    /// # Security Warning
    /// Use with caution! This exposes the sensitive data.
    /// Avoid logging or displaying the returned value.
    pub fn expose_secret(&self) -> &str {
        &self.inner
    }

    /// Check if the secret is empty
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Get the length of the secret
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

// Custom Serialize implementation to prevent accidental serialization
impl Serialize for SecretString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Never serialize the actual secret
        serializer.serialize_str("[REDACTED]")
    }
}

// Custom Deserialize implementation
impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(SecretString::new(s))
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        SecretString::new(s)
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        SecretString::from_str(s)
    }
}

/// Provider API keys collection
///
/// Stores API keys for various LLM providers with secure zeroization.
#[derive(Debug, Clone)]
pub struct ProviderSecrets {
    pub anthropic: Option<SecretString>,
    pub openai: Option<SecretString>,
    pub gemini: Option<SecretString>,
    pub azure: Option<SecretString>,
}

impl ProviderSecrets {
    /// Create a new empty ProviderSecrets
    pub fn new() -> Self {
        Self {
            anthropic: None,
            openai: None,
            gemini: None,
            azure: None,
        }
    }

    /// Load all provider secrets from environment variables
    pub fn from_env() -> Self {
        Self {
            anthropic: SecretString::from_env_optional("ANTHROPIC_API_KEY"),
            openai: SecretString::from_env_optional("OPENAI_API_KEY"),
            gemini: SecretString::from_env_optional("GEMINI_API_KEY"),
            azure: SecretString::from_env_optional("AZURE_OPENAI_KEY"),
        }
    }

    /// Load all provider secrets with fallback (keyring → env → none)
    ///
    /// This is the recommended method. It tries to load each API key from:
    /// 1. OS keyring (most secure, persistent)
    /// 2. Environment variable (temporary)
    /// 3. None (not configured)
    pub fn load_with_fallback() -> Self {
        Self {
            anthropic: SecretString::load_with_fallback("anthropic_api_key", "ANTHROPIC_API_KEY"),
            openai: SecretString::load_with_fallback("openai_api_key", "OPENAI_API_KEY"),
            gemini: SecretString::load_with_fallback("gemini_api_key", "GEMINI_API_KEY"),
            azure: SecretString::load_with_fallback("azure_openai_key", "AZURE_OPENAI_KEY"),
        }
    }

    /// Save a provider API key to OS keyring
    pub fn save_to_keyring(&self, provider: &str) -> Result<()> {
        let (secret, key_name) = match provider {
            "anthropic" => (self.anthropic.as_ref(), "anthropic_api_key"),
            "openai" => (self.openai.as_ref(), "openai_api_key"),
            "gemini" => (self.gemini.as_ref(), "gemini_api_key"),
            "azure" => (self.azure.as_ref(), "azure_openai_key"),
            _ => anyhow::bail!("Unknown provider: {}", provider),
        };

        let secret =
            secret.ok_or_else(|| anyhow::anyhow!("No API key configured for {}", provider))?;
        secret.save_to_keyring(key_name)?;

        Ok(())
    }

    /// Delete a provider API key from OS keyring
    pub fn delete_from_keyring(provider: &str) -> Result<()> {
        let key_name = match provider {
            "anthropic" => "anthropic_api_key",
            "openai" => "openai_api_key",
            "gemini" => "gemini_api_key",
            "azure" => "azure_openai_key",
            _ => anyhow::bail!("Unknown provider: {}", provider),
        };

        SecretString::delete_from_keyring(key_name)?;
        Ok(())
    }

    /// Check if any secrets are configured
    pub fn has_any(&self) -> bool {
        self.anthropic.is_some()
            || self.openai.is_some()
            || self.gemini.is_some()
            || self.azure.is_some()
    }

    /// Get the number of configured secrets
    pub fn count(&self) -> usize {
        let mut count = 0;
        if self.anthropic.is_some() {
            count += 1;
        }
        if self.openai.is_some() {
            count += 1;
        }
        if self.gemini.is_some() {
            count += 1;
        }
        if self.azure.is_some() {
            count += 1;
        }
        count
    }
}

impl Default for ProviderSecrets {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_string_creation() {
        let secret = SecretString::from_str("my-secret-key");
        assert_eq!(secret.expose_secret(), "my-secret-key");
        assert_eq!(secret.len(), 13);
        assert!(!secret.is_empty());
    }

    #[test]
    fn test_secret_string_debug() {
        let secret = SecretString::from_str("my-secret-key");
        let debug_output = format!("{:?}", secret);
        assert_eq!(debug_output, "[REDACTED]");
        assert!(!debug_output.contains("my-secret-key"));
    }

    #[test]
    fn test_secret_string_display() {
        let secret = SecretString::from_str("my-secret-key");
        let display_output = format!("{}", secret);
        assert_eq!(display_output, "[REDACTED]");
        assert!(!display_output.contains("my-secret-key"));
    }

    #[test]
    fn test_provider_secrets_empty() {
        let secrets = ProviderSecrets::new();
        assert!(!secrets.has_any());
        assert_eq!(secrets.count(), 0);
    }

    #[test]
    fn test_provider_secrets_with_keys() {
        let secrets = ProviderSecrets {
            anthropic: Some(SecretString::from_str("key1")),
            openai: Some(SecretString::from_str("key2")),
            gemini: None,
            azure: None,
        };
        assert!(secrets.has_any());
        assert_eq!(secrets.count(), 2);
    }

    #[test]
    fn test_secret_string_from_env_missing() {
        // Test that a non-existent env var returns an error
        let result = SecretString::from_env("OPENCRABS_TEST_NONEXISTENT_KEY_12345");
        assert!(result.is_err());
    }

    #[test]
    fn test_secret_string_from_env_optional_missing() {
        // Test with non-existent variable returns None
        let secret = SecretString::from_env_optional("OPENCRABS_TEST_NONEXISTENT_KEY_12345");
        assert!(secret.is_none());
    }

    #[test]
    fn test_secret_string_serialize() {
        let secret = SecretString::from_str("my-secret-key");
        let serialized = serde_json::to_string(&secret).unwrap();
        assert_eq!(serialized, "\"[REDACTED]\"");
        assert!(!serialized.contains("my-secret-key"));
    }
}
