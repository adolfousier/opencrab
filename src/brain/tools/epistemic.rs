//! Epistemic Engine — Belief Tracking with Confidence Levels
//!
//! Tracks beliefs (facts, decisions, context) with confidence levels and
//! source attribution. Implements decay logic and contradiction detection.
//!
//! Design:
//! - Confidence levels: verified, inferred, uncertain, contradicted
//! - Source attribution: every belief tagged with origin
//! - Decay: unverified beliefs lose confidence after 30 days
//! - Contradiction detection: new fact conflicts existing belief → flagged
//! - Storage: ~/.opencrabs/brain/epistemic/beliefs.toml
//!
//! Config: ralph_loop.toml [epistemic] section (already exists)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Confidence levels for beliefs, ordered from most to least certain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Conflicts with another belief — needs resolution (lowest confidence)
    Contradicted,
    /// Not yet verified, assumed true
    Uncertain,
    /// Derived from other beliefs or logical inference
    Inferred,
    /// Confirmed by user or system verification (highest confidence)
    Verified,
}

impl Confidence {
    /// Decay confidence by one level. Verified beliefs don't decay.
    pub fn decay(self) -> Self {
        match self {
            Confidence::Verified => Confidence::Verified,
            Confidence::Inferred => Confidence::Uncertain,
            Confidence::Uncertain => Confidence::Contradicted,
            Confidence::Contradicted => Confidence::Contradicted,
        }
    }

    /// Human-readable label
    pub fn label(&self) -> &'static str {
        match self {
            Confidence::Verified => "verified",
            Confidence::Inferred => "inferred",
            Confidence::Uncertain => "uncertain",
            Confidence::Contradicted => "contradicted",
        }
    }
}

/// Source attribution for a belief.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    /// Who/what provided this belief (e.g. "user:adolfo", "inference", "session:abc123")
    pub origin: String,
    /// When the belief was first recorded
    pub recorded_at: DateTime<Utc>,
    /// When the belief was last verified
    pub last_verified: DateTime<Utc>,
}

/// A single belief with confidence and source tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Belief {
    /// Unique key for this belief (e.g. "memory:truelens:staging_ip")
    pub key: String,
    /// The belief value (e.g. "159.65.49.225")
    pub value: String,
    /// Current confidence level
    pub confidence: Confidence,
    /// Source attribution
    pub source: Source,
    /// Optional notes or context
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// The epistemic store — all tracked beliefs.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct EpistemicStore {
    /// Map of belief key → belief
    #[serde(default)]
    pub beliefs: HashMap<String, Belief>,
    /// Schema version for future migrations
    #[serde(default = "default_version")]
    pub version: u32,
}

fn default_version() -> u32 {
    1
}

impl EpistemicStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self {
            beliefs: HashMap::new(),
            version: 1,
        }
    }

    /// Add or update a belief. If the key exists and the value differs,
    /// the old belief is marked as contradicted.
    pub fn add_belief(
        &mut self,
        key: &str,
        value: &str,
        confidence: Confidence,
        origin: &str,
    ) -> ContradictionResult {
        let now = Utc::now();

        // Check for contradiction with existing belief.
        // Clone first to avoid borrow conflict (immutable get + mutable insert).
        let existing = self.beliefs.get(key).cloned();
        if let Some(existing) = existing
            && existing.value != value
            && existing.confidence != Confidence::Contradicted
        {
            // Extract old_value BEFORE mutable borrow
            let old_value = existing.value.clone();

            // Mark existing belief as contradicted
            let mut contradicted = existing.clone();
            contradicted.confidence = Confidence::Contradicted;
            contradicted.notes = Some(format!(
                "Contradicted by new value '{}' from {} at {}",
                value,
                origin,
                now.format("%Y-%m-%d %H:%M:%S UTC")
            ));
            self.beliefs.insert(
                format!("{}:contradicted:{}", key, now.timestamp()),
                contradicted,
            );

            // Insert new belief
            let belief = Belief {
                key: key.to_string(),
                value: value.to_string(),
                confidence,
                source: Source {
                    origin: origin.to_string(),
                    recorded_at: now,
                    last_verified: now,
                },
                notes: None,
            };
            self.beliefs.insert(key.to_string(), belief);

            return ContradictionResult::Contradicted {
                old_value,
                new_value: value.to_string(),
            };
        }

        // No contradiction — insert or update
        let belief = Belief {
            key: key.to_string(),
            value: value.to_string(),
            confidence,
            source: Source {
                origin: origin.to_string(),
                recorded_at: now,
                last_verified: now,
            },
            notes: None,
        };
        self.beliefs.insert(key.to_string(), belief);

        ContradictionResult::NoContradiction
    }

    /// Get a belief by key.
    pub fn get_belief(&self, key: &str) -> Option<&Belief> {
        self.beliefs.get(key)
    }

    /// Re-verify a belief (updates last_verified timestamp).
    pub fn verify_belief(&mut self, key: &str) -> bool {
        if let Some(belief) = self.beliefs.get_mut(key) {
            belief.source.last_verified = Utc::now();
            belief.confidence = Confidence::Verified;
            true
        } else {
            false
        }
    }

    /// Apply decay logic: beliefs not verified within `decay_days` drop
    /// one confidence level. Verified beliefs are immune.
    pub fn apply_decay(&mut self, decay_days: i64) -> Vec<String> {
        let now = Utc::now();
        let mut decayed = Vec::new();

        for belief in self.beliefs.values_mut() {
            if belief.confidence == Confidence::Verified {
                continue; // Verified beliefs don't decay
            }

            let age_days = (now - belief.source.last_verified).num_days();
            if age_days >= decay_days {
                let old = belief.confidence;
                belief.confidence = belief.confidence.decay();
                if belief.confidence != old {
                    decayed.push(format!(
                        "{}: {} → {} ({} days since verification)",
                        belief.key,
                        old.label(),
                        belief.confidence.label(),
                        age_days
                    ));
                }
            }
        }

        decayed
    }

    /// List beliefs filtered by confidence level.
    pub fn list_by_confidence(&self, confidence: Confidence) -> Vec<&Belief> {
        self.beliefs
            .values()
            .filter(|b| b.confidence == confidence)
            .collect()
    }

    /// List all contradicted beliefs (for review).
    pub fn list_contradictions(&self) -> Vec<&Belief> {
        self.list_by_confidence(Confidence::Contradicted)
    }

    /// Save the store to disk.
    pub fn save(&self, path: &PathBuf) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, content)
    }

    /// Load the store from disk. Returns empty store if file missing.
    pub fn load(path: &PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(store) => store,
                Err(e) => {
                    tracing::warn!("Epistemic store parse error: {}", e);
                    Self::new()
                }
            },
            Err(_) => Self::new(),
        }
    }
}

/// Result of adding a belief — indicates if a contradiction was detected.
#[derive(Debug, Clone, PartialEq)]
pub enum ContradictionResult {
    /// No contradiction — belief added/updated normally
    NoContradiction,
    /// Contradiction detected — old belief marked as contradicted
    Contradicted {
        old_value: String,
        new_value: String,
    },
}

/// Get the epistemic store path.
fn epistemic_store_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".opencrabs/brain/epistemic/beliefs.toml"))
}

/// Global epistemic store (cached for session lifetime).
static STORE: OnceLock<std::sync::Mutex<EpistemicStore>> = OnceLock::new();

fn get_store() -> &'static std::sync::Mutex<EpistemicStore> {
    STORE.get_or_init(|| {
        let path = epistemic_store_path().expect("home dir must exist");
        std::sync::Mutex::new(EpistemicStore::load(&path))
    })
}

/// Add a belief to the global store. Returns contradiction result.
pub fn add_belief(
    key: &str,
    value: &str,
    confidence: Confidence,
    origin: &str,
) -> ContradictionResult {
    let store = get_store();
    let mut guard = store.lock().expect("epistemic store lock poisoned");
    let result = guard.add_belief(key, value, confidence, origin);

    // Auto-save after modification
    if let Some(path) = epistemic_store_path()
        && let Err(e) = guard.save(&path)
    {
        tracing::warn!("Failed to save epistemic store: {}", e);
    }

    result
}

/// Get a belief from the global store.
pub fn get_belief(key: &str) -> Option<Belief> {
    let store = get_store();
    let guard = store.lock().expect("epistemic store lock poisoned");
    guard.get_belief(key).cloned()
}

/// Verify a belief in the global store.
pub fn verify_belief(key: &str) -> bool {
    let store = get_store();
    let mut guard = store.lock().expect("epistemic store lock poisoned");
    let result = guard.verify_belief(key);

    if result
        && let Some(path) = epistemic_store_path()
        && let Err(e) = guard.save(&path)
    {
        tracing::warn!("Failed to save epistemic store: {}", e);
    }

    result
}

/// Apply decay to the global store. Returns list of decayed beliefs.
pub fn apply_decay(decay_days: i64) -> Vec<String> {
    let store = get_store();
    let mut guard = store.lock().expect("epistemic store lock poisoned");
    let decayed = guard.apply_decay(decay_days);

    if !decayed.is_empty()
        && let Some(path) = epistemic_store_path()
        && let Err(e) = guard.save(&path)
    {
        tracing::warn!("Failed to save epistemic store: {}", e);
    }

    decayed
}

/// List all contradicted beliefs in the global store.
pub fn list_contradictions() -> Vec<Belief> {
    let store = get_store();
    let guard = store.lock().expect("epistemic store lock poisoned");
    guard.list_contradictions().into_iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_ordering() {
        assert!(Confidence::Verified > Confidence::Inferred);
        assert!(Confidence::Inferred > Confidence::Uncertain);
        assert!(Confidence::Uncertain > Confidence::Contradicted);
    }

    #[test]
    fn test_confidence_decay() {
        assert_eq!(Confidence::Verified.decay(), Confidence::Verified);
        assert_eq!(Confidence::Inferred.decay(), Confidence::Uncertain);
        assert_eq!(Confidence::Uncertain.decay(), Confidence::Contradicted);
        assert_eq!(Confidence::Contradicted.decay(), Confidence::Contradicted);
    }

    #[test]
    fn test_add_belief_no_contradiction() {
        let mut store = EpistemicStore::new();
        let result = store.add_belief("test:key", "value1", Confidence::Verified, "test");
        assert_eq!(result, ContradictionResult::NoContradiction);
        assert_eq!(store.get_belief("test:key").unwrap().value, "value1");
    }

    #[test]
    fn test_add_belief_contradiction() {
        let mut store = EpistemicStore::new();
        store.add_belief("test:key", "value1", Confidence::Verified, "test");
        let result = store.add_belief("test:key", "value2", Confidence::Inferred, "test2");

        assert!(matches!(result, ContradictionResult::Contradicted { .. }));

        // Old belief should be marked as contradicted
        let contradicted: Vec<_> = store.list_contradictions();
        assert_eq!(contradicted.len(), 1);
        assert_eq!(contradicted[0].value, "value1");

        // New belief should be active
        assert_eq!(store.get_belief("test:key").unwrap().value, "value2");
    }

    #[test]
    fn test_verify_belief() {
        let mut store = EpistemicStore::new();
        store.add_belief("test:key", "value", Confidence::Uncertain, "test");
        assert!(store.verify_belief("test:key"));
        assert_eq!(
            store.get_belief("test:key").unwrap().confidence,
            Confidence::Verified
        );
    }

    #[test]
    fn test_decay_logic() {
        let mut store = EpistemicStore::new();

        // Add a belief with old last_verified
        let mut belief = Belief {
            key: "test:old".to_string(),
            value: "old_value".to_string(),
            confidence: Confidence::Inferred,
            source: Source {
                origin: "test".to_string(),
                recorded_at: Utc::now() - chrono::Duration::days(45),
                last_verified: Utc::now() - chrono::Duration::days(45),
            },
            notes: None,
        };
        store.beliefs.insert("test:old".to_string(), belief.clone());

        // Add a recent belief
        belief.key = "test:recent".to_string();
        belief.confidence = Confidence::Inferred;
        belief.source.last_verified = Utc::now();
        store.beliefs.insert("test:recent".to_string(), belief);

        // Apply decay with 30-day threshold
        let decayed = store.apply_decay(30);

        // Only the old belief should decay
        assert_eq!(decayed.len(), 1);
        assert!(decayed[0].contains("test:old"));
        assert_eq!(
            store.get_belief("test:old").unwrap().confidence,
            Confidence::Uncertain
        );
        assert_eq!(
            store.get_belief("test:recent").unwrap().confidence,
            Confidence::Inferred
        );
    }

    #[test]
    fn test_verified_beliefs_dont_decay() {
        let mut store = EpistemicStore::new();

        let belief = Belief {
            key: "test:verified".to_string(),
            value: "verified_value".to_string(),
            confidence: Confidence::Verified,
            source: Source {
                origin: "test".to_string(),
                recorded_at: Utc::now() - chrono::Duration::days(100),
                last_verified: Utc::now() - chrono::Duration::days(100),
            },
            notes: None,
        };
        store.beliefs.insert("test:verified".to_string(), belief);

        let decayed = store.apply_decay(30);
        assert!(decayed.is_empty());
        assert_eq!(
            store.get_belief("test:verified").unwrap().confidence,
            Confidence::Verified
        );
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut store = EpistemicStore::new();
        store.add_belief("test:key", "value", Confidence::Inferred, "test:origin");

        let toml_str = toml::to_string_pretty(&store).unwrap();
        let loaded: EpistemicStore = toml::from_str(&toml_str).unwrap();

        assert_eq!(loaded.get_belief("test:key").unwrap().value, "value");
        assert_eq!(
            loaded.get_belief("test:key").unwrap().confidence,
            Confidence::Inferred
        );
    }
}
