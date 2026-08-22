//! Profile List Tool
//!
//! Agent-callable enumeration of OpenCrabs profiles and their A2A endpoints
//! (#1161). Reads each profile's own `config.toml` `[a2a]` section and
//! projects ONLY the safe fields - `api_key` is never surfaced, enforced by
//! construction (the render path has no access to it) and by test.

use super::error::{Result, ToolError};
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolHints, ToolResult};
use crate::config::profile::{home_for_profile, list_profiles};
use crate::config::types::A2aConfig;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

/// Subset of a profile's config.toml we are allowed to read.
#[derive(Debug, Default, Deserialize)]
struct ProfileConfigFile {
    #[serde(default)]
    a2a: A2aConfig,
}

/// One roster row: profile identity plus its parsed `[a2a]` section.
pub(crate) struct ProfileA2aRow {
    pub name: String,
    pub description: Option<String>,
    pub a2a: A2aConfig,
    /// Whether the profile's config.toml actually contained an `[a2a]` table.
    pub config_found: bool,
}

/// Load a named profile's `[a2a]` config from disk.
///
/// Missing config.toml or missing `[a2a]` table yields defaults
/// (`enabled = false`, port 18790) with `config_found = false`.
pub(crate) fn load_profile_a2a(name: &str) -> Result<(A2aConfig, bool)> {
    let path = home_for_profile(Some(name)).join("config.toml");
    if !path.exists() {
        return Ok((A2aConfig::default(), false));
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| ToolError::Execution(format!("failed to read {}: {e}", path.display())))?;
    let parsed: ProfileConfigFile = toml::from_str(&content)
        .map_err(|e| ToolError::Execution(format!("failed to parse {}: {e}", path.display())))?;
    // Distinguish "table present" from "whole file defaulted" for the roster.
    let found = content.contains("[a2a]");
    Ok((parsed.a2a, found))
}

/// The URL other crabs should use to reach this profile's gateway.
///
/// Precedence per #1161: `advertise_url` when set (multi-host reachability),
/// else `http://{bind}:{port}` (same-box default).
pub(crate) fn effective_a2a_url(cfg: &A2aConfig) -> String {
    match cfg.advertise_url.as_deref() {
        Some(u) if !u.trim().is_empty() => u.trim().trim_end_matches('/').to_string(),
        _ => format!("http://{}:{}", cfg.bind, cfg.port),
    }
}

/// Render the roster. Pure so tests can pin output shape, the api_key ban,
/// and collision warnings without touching a real profile directory.
///
/// The function only receives projected rows; even if a caller smuggled an
/// api_key into `row.a2a`, nothing here reads it.
pub(crate) fn render_roster(rows: &[ProfileA2aRow]) -> String {
    let mut lines = vec![format!("Profiles ({}):", rows.len())];
    let mut enabled_by_port: std::collections::BTreeMap<u16, Vec<String>> =
        std::collections::BTreeMap::new();

    for row in rows {
        let desc = row.description.as_deref().unwrap_or("").trim();
        let mut line = if desc.is_empty() {
            format!("- {}", row.name)
        } else {
            format!("- {} - {}", row.name, desc)
        };
        if row.a2a.enabled {
            let mut a2a = format!("  a2a: enabled on {}:{}", row.a2a.bind, row.a2a.port);
            if let Some(adv) = row.a2a.advertise_url.as_deref()
                && !adv.trim().is_empty()
            {
                a2a.push_str(&format!(" (advertise_url: {adv})"));
            }
            line.push('\n');
            line.push_str(&a2a);
            enabled_by_port
                .entry(row.a2a.port)
                .or_default()
                .push(row.name.clone());
        } else {
            let note = if row.config_found {
                ""
            } else {
                " (no [a2a] in config.toml)"
            };
            line.push_str(&format!("\n  a2a: disabled{note}"));
        }
        lines.push(line);
    }

    for (port, names) in &enabled_by_port {
        if names.len() > 1 {
            let joined = match names.as_slice() {
                [a, b] => format!("{a} and {b}"),
                _ => names.join(", "),
            };
            lines.push(format!(
                "warning: {joined} both on {port} - the second to start will fail to bind"
            ));
        }
    }

    lines.join("\n")
}

/// Tool exposing the profile roster to agents.
#[derive(Default)]
pub struct ProfileListTool;

impl ProfileListTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ProfileListTool {
    fn name(&self) -> &str {
        "profile_list"
    }

    fn description(&self) -> &str {
        "List OpenCrabs profiles and their A2A gateway endpoints (enabled \
         state, bind address, port, advertise_url when set). Use a returned \
         profile name as the 'profile' parameter of a2a_send instead of a raw \
         URL. Flags enabled profiles that share a port."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadFiles]
    }

    fn hints(&self) -> ToolHints {
        ToolHints {
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
        }
    }

    async fn execute(&self, _input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        let profiles = list_profiles()
            .map_err(|e| ToolError::Execution(format!("failed to list profiles: {e}")))?;
        let mut rows = Vec::with_capacity(profiles.len());
        for entry in &profiles {
            let (a2a, config_found) = load_profile_a2a(&entry.name)?;
            rows.push(ProfileA2aRow {
                name: entry.name.clone(),
                description: entry.description.clone(),
                a2a,
                config_found,
            });
        }
        Ok(ToolResult::success(render_roster(&rows)))
    }
}
