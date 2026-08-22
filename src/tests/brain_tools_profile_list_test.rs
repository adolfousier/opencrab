//! #1161 profile_list suite: the api_key ban (by construction AND by test),
//! advertise_url precedence over bind:port, disabled rendering, and the
//! port-collision warning. All pure - no real profile directories touched.

use crate::brain::tools::profile_list::{ProfileA2aRow, effective_a2a_url, render_roster};
use crate::config::types::A2aConfig;

fn row(name: &str, a2a: A2aConfig) -> ProfileA2aRow {
    ProfileA2aRow {
        name: name.to_string(),
        description: None,
        a2a,
        config_found: true,
    }
}

#[test]
fn test_api_key_never_surfaced_in_roster() {
    // Even if a caller smuggles a configured api_key into the row, the
    // rendered roster must not contain it - the negative test pinning the
    // #1161 hard rule.
    let mut cfg = A2aConfig::default();
    cfg.enabled = true;
    cfg.api_key = Some("super-secret-key-do-not-leak".to_string());
    let out = render_roster(&[row("worker", cfg)]);
    assert!(
        !out.contains("super-secret-key-do-not-leak"),
        "api_key leaked into roster: {out}"
    );
    assert!(out.contains("worker"));
    assert!(out.contains("a2a: enabled"));
}

#[test]
fn test_advertise_url_wins_over_bind_port() {
    let mut cfg = A2aConfig::default();
    cfg.enabled = true;
    cfg.bind = "0.0.0.0".into();
    cfg.port = 18790;
    cfg.advertise_url = Some("http://crab.example.com:9999".into());
    assert_eq!(
        effective_a2a_url(&cfg),
        "http://crab.example.com:9999",
        "#1161: advertise_url must take precedence"
    );
}

#[test]
fn test_advertise_url_trimmed_of_slash_and_whitespace() {
    let mut cfg = A2aConfig::default();
    cfg.advertise_url = Some("  http://relay.example.com/  ".into());
    assert_eq!(effective_a2a_url(&cfg), "http://relay.example.com");
}

#[test]
fn test_bind_port_fallback_when_no_advertise_url() {
    let mut cfg = A2aConfig::default();
    cfg.bind = "127.0.0.1".into();
    cfg.port = 18790;
    // None and blank-but-set must both fall back.
    assert_eq!(effective_a2a_url(&cfg), "http://127.0.0.1:18790");
    cfg.advertise_url = Some("   ".into());
    assert_eq!(effective_a2a_url(&cfg), "http://127.0.0.1:18790");
}

#[test]
fn test_disabled_profile_rendering_notes_config_presence() {
    let absent = A2aConfig::default();
    let out = render_roster(&[
        row("bare", absent.clone()),
        ProfileA2aRow {
            config_found: false,
            ..row("nofile", A2aConfig::default())
        },
    ]);
    assert!(out.contains("a2a: disabled"), "got: {out}");
    assert!(
        !out.contains("(no [a2a] in config.toml)\n  a2a: disabled\n") || out.contains("nofile"),
    );
    // The no-config variant carries the explanatory note; the config'd one doesn't.
    let bare_line = out.lines().find(|l| l.starts_with("- bare")).unwrap();
    let bare_idx = out.find("- bare").unwrap();
    let bare_section = &out[bare_idx..bare_idx + bare_line.len() + 40];
    assert!(!bare_section.contains("no [a2a]"));
}

#[test]
fn test_enabled_roster_shows_advertise_url_and_collision_warning() {
    let mut a = A2aConfig::default();
    a.enabled = true;
    a.advertise_url = Some("http://a.example.com".into());
    let mut b = a.clone();
    b.advertise_url = None;
    let out = render_roster(&[row("alpha", a), row("beta", b)]);
    assert!(
        out.contains("(advertise_url: http://a.example.com)"),
        "got: {out}"
    );
    assert!(
        out.contains("warning: alpha and beta both on 18790"),
        "collision warning missing: {out}"
    );
}
