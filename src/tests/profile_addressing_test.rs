//! #1161 profile addressing suite: `profile_list` tool, `a2a_send` name
//! resolution, port-collision warnings, agent-card identity.

use crate::brain::tools::a2a_send::{A2aSendTool, profile_disabled_error};
use crate::brain::tools::profile_list::{
    ProfileA2aRow, ProfileListTool, effective_a2a_url, render_roster,
};
use crate::brain::tools::{Tool, ToolExecutionContext};
use crate::config::types::A2aConfig;

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(uuid::Uuid::new_v4())
}

fn row(name: &str, cfg: A2aConfig) -> ProfileA2aRow {
    ProfileA2aRow {
        name: name.to_string(),
        description: None,
        a2a: cfg,
        config_found: true,
    }
}

#[test]
fn schema_exposes_profile_param() {
    let tool = A2aSendTool::new();
    let schema = tool.input_schema();
    let props = schema["properties"].as_object().unwrap();
    assert!(props.contains_key("profile"));
    assert!(props.contains_key("url"));
    // url is no longer hard-required at the schema level; exactly-one-of
    // url/profile is enforced in execute().
}

/// Hard rule from #1161: api_key must never appear in profile_list output.
#[test]
fn profile_list_never_surfaces_api_key() {
    let secret = A2aConfig {
        enabled: true,
        api_key: Some("sk-super-secret-value-123".to_string()),
        ..Default::default()
    };
    let out = render_roster(&[row("truelens", secret)]);
    assert!(!out.contains("api_key"), "leaked field name: {out}");
    assert!(
        !out.contains("sk-super-secret-value-123"),
        "leaked key value: {out}"
    );
    assert!(out.contains("truelens"));
    assert!(out.contains("enabled"));
}

/// Resolution precedence (#1161): advertise_url wins, bind:port is fallback.
#[test]
fn effective_url_prefers_advertise_url_over_bind_port() {
    let adv = A2aConfig {
        enabled: true,
        advertise_url: Some("http://crab.example.ts.net:9999/".to_string()),
        ..Default::default()
    };
    assert_eq!(effective_a2a_url(&adv), "http://crab.example.ts.net:9999");

    let plain = A2aConfig {
        enabled: true,
        ..Default::default()
    };
    assert_eq!(effective_a2a_url(&plain), "http://127.0.0.1:18790");
}

#[tokio::test]
async fn a2a_send_rejects_url_and_profile_together() {
    let tool = A2aSendTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "action": "discover",
                "url": "http://127.0.0.1:18790",
                "profile": "ops"
            }),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(!result.success);
    let err = result.error.unwrap_or_default();
    assert!(err.contains("mutually exclusive"), "was: {err}");
}

#[tokio::test]
async fn a2a_send_requires_a_target() {
    let tool = A2aSendTool::new();
    let result = tool
        .execute(serde_json::json!({"action": "discover"}), &ctx())
        .await
        .unwrap();
    assert!(!result.success);
    let err = result.error.unwrap_or_default();
    assert!(err.contains("'url' or 'profile'"), "was: {err}");
}

/// #1161 exact wording: names the profile AND the fix.
#[test]
fn disabled_target_error_names_profile_and_fix() {
    let err = profile_disabled_error("truelens");
    assert!(err.contains("'truelens'"), "was: {err}");
    assert!(err.contains("enabled=true"), "was: {err}");
    assert!(err.contains("has a2a disabled"), "wording drifted: {err}");
}

/// Collision pre-flight (#1161): only ENABLED profiles sharing a port warn.
#[test]
fn collision_warning_fires_for_enabled_profiles_sharing_port() {
    let a = A2aConfig {
        enabled: true,
        ..Default::default()
    };
    let b = A2aConfig {
        enabled: true,
        ..Default::default()
    };
    let out = render_roster(&[row("ops", a), row("truelens", b)]);
    assert!(
        out.contains("warning: ops and truelens both on 18790"),
        "was: {out}"
    );
    assert!(out.contains("fail to bind"));

    // Disabled profiles never collide.
    let off = A2aConfig {
        enabled: false,
        ..Default::default()
    };
    let on = A2aConfig {
        enabled: true,
        ..Default::default()
    };
    let out2 = render_roster(&[row("ops", on), row("quiet", off)]);
    assert!(!out2.contains("warning:"), "was: {out2}");
}

/// Agent card identity (#1161): discover answers "who is this".
#[test]
fn agent_card_carries_profile_name() {
    let card = crate::config::profile::with_profile_home(Some("truelens"), || {
        crate::a2a::agent_card::build_agent_card("127.0.0.1", 18790, None)
    });
    assert!(
        card.name.contains("'truelens'"),
        "card name was: {}",
        card.name
    );
    assert!(card.name.starts_with("OpenCrabs "));
    assert!(card.name.contains(crate::VERSION));
}

/// The tool itself registers with the expected name and zero-param schema.
#[test]
fn profile_list_tool_shape() {
    let tool = ProfileListTool::new();
    assert_eq!(tool.name(), "profile_list");
    let schema = tool.input_schema();
    assert_eq!(schema["properties"].as_object().unwrap().len(), 0);
    assert!(tool.hints().read_only);
}
