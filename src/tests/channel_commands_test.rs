//! Tests for channel commands: format_number, format_help, provider helpers,
//! and user command matching.

use crate::brain::commands::UserCommand;
use crate::channels::commands::{
    ChannelCommand, format_help, format_number, match_user_command_inner, normalize_provider_name,
    provider_display_name, provider_names_match,
};

// ── format_number ─────────────────────────────────────────────────────

#[test]
fn format_number_small() {
    assert_eq!(format_number(0), "0");
    assert_eq!(format_number(1), "1");
    assert_eq!(format_number(999), "999");
}

#[test]
fn format_number_thousands() {
    assert_eq!(format_number(1_000), "1.0K");
    assert_eq!(format_number(1_500), "1.5K");
    assert_eq!(format_number(999_999), "1000.0K");
}

#[test]
fn format_number_millions() {
    assert_eq!(format_number(1_000_000), "1.0M");
    assert_eq!(format_number(2_500_000), "2.5M");
    assert_eq!(format_number(123_456_789), "123.5M");
}

// ── format_help ───────────────────────────────────────────────────────

#[test]
fn format_help_contains_all_commands() {
    let help = format_help();
    for cmd in [
        "/evolve",
        "/help",
        "/models",
        "/new",
        "/sessions",
        "/stop",
        "/usage",
    ] {
        assert!(help.contains(cmd), "help text missing {}", cmd);
    }
}

#[test]
fn format_help_is_alphabetical() {
    let help = format_help();
    let builtin_section = help.split("Custom Commands").next().unwrap_or(&help);
    let commands: Vec<&str> = builtin_section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim().strip_prefix('`')?;
            let cmd = trimmed.split('`').next()?;
            if cmd.starts_with('/') {
                Some(cmd.split_whitespace().next().unwrap_or(cmd))
            } else {
                None
            }
        })
        .collect();
    let mut sorted = commands.clone();
    sorted.sort();
    assert_eq!(
        commands, sorted,
        "built-in help commands are not alphabetical"
    );
}

// ── provider_display_name ─────────────────────────────────────────────

#[test]
fn provider_display_name_known() {
    assert_eq!(provider_display_name("anthropic"), "Anthropic");
    assert_eq!(provider_display_name("openai"), "OpenAI");
    assert_eq!(provider_display_name("github"), "GitHub Copilot");
    assert_eq!(provider_display_name("openrouter"), "OpenRouter");
    assert_eq!(provider_display_name("minimax"), "MiniMax");
    assert_eq!(provider_display_name("gemini"), "Gemini");
}

#[test]
fn provider_aliases_normalize_and_match() {
    assert_eq!(normalize_provider_name("GitHub Copilot"), "github");
    assert_eq!(normalize_provider_name("Google Gemini"), "gemini");
    assert_eq!(
        normalize_provider_name("custom(DeepSeek)"),
        "custom:deepseek"
    );
    assert!(provider_names_match("custom:deepseek", "deepseek"));
}

#[test]
fn provider_display_name_custom() {
    assert_eq!(provider_display_name("custom:deepseek"), "deepseek");
    assert_eq!(provider_display_name("custom:local-llm"), "local-llm");
}

#[test]
fn provider_display_name_unknown() {
    assert_eq!(provider_display_name("mystery"), "mystery");
}

// ── match_user_command_inner ──────────────────────────────────────────

fn make_cmd(name: &str, action: &str, prompt: &str) -> UserCommand {
    UserCommand {
        name: name.to_string(),
        description: String::new(),
        action: action.to_string(),
        prompt: prompt.to_string(),
    }
}

fn variant_name(cmd: &ChannelCommand) -> &'static str {
    match cmd {
        ChannelCommand::Compact => "Compact",
        ChannelCommand::Help(_) => "Help",
        ChannelCommand::Usage(_) => "Usage",
        ChannelCommand::Models(_) => "Models",
        ChannelCommand::NewSession => "NewSession",
        ChannelCommand::Sessions(_) => "Sessions",
        ChannelCommand::Stop => "Stop",
        ChannelCommand::UserPrompt(_) => "UserPrompt",
        ChannelCommand::UserSystem(_) => "UserSystem",
        ChannelCommand::Doctor => "Doctor",
        ChannelCommand::Evolve => "Evolve",
        ChannelCommand::NotACommand => "NotACommand",
    }
}

#[test]
fn user_command_prompt_no_args() {
    let cmds = vec![make_cmd(
        "/credits",
        "prompt",
        "Check my OpenRouter credits",
    )];
    match match_user_command_inner("/credits", &cmds, &[]) {
        ChannelCommand::UserPrompt(p) => assert_eq!(p, "Check my OpenRouter credits"),
        other => panic!("expected UserPrompt, got {:?}", variant_name(&other)),
    }
}

#[test]
fn user_command_prompt_with_args() {
    let cmds = vec![make_cmd("/deploy", "prompt", "Deploy the service")];
    match match_user_command_inner("/deploy staging --dry-run", &cmds, &[]) {
        ChannelCommand::UserPrompt(p) => assert_eq!(p, "Deploy the service staging --dry-run"),
        other => panic!("expected UserPrompt, got {:?}", variant_name(&other)),
    }
}

#[test]
fn user_command_system_action() {
    let cmds = vec![make_cmd("/info", "system", "OpenCrabs v0.2")];
    match match_user_command_inner("/info", &cmds, &[]) {
        ChannelCommand::UserSystem(t) => assert_eq!(t, "OpenCrabs v0.2"),
        other => panic!("expected UserSystem, got {:?}", variant_name(&other)),
    }
}

#[test]
fn user_command_unknown_falls_through() {
    let cmds = vec![make_cmd("/credits", "prompt", "Check credits")];
    assert!(matches!(
        match_user_command_inner("/unknown", &cmds, &[]),
        ChannelCommand::NotACommand
    ));
}

#[test]
fn user_command_empty_list() {
    assert!(matches!(
        match_user_command_inner("/anything", &[], &[]),
        ChannelCommand::NotACommand
    ));
}

#[test]
fn user_command_default_action_is_prompt() {
    let cmds = vec![make_cmd("/test", "whatever", "test prompt")];
    assert!(matches!(
        match_user_command_inner("/test", &cmds, &[]),
        ChannelCommand::UserPrompt(_)
    ));
}
