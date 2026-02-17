//! Onboarding Wizard Rendering
//!
//! Render functions for each step of the onboarding wizard.

use super::onboarding::{
    AuthField, BrainField, DiscordField, HealthStatus, MessagingField, OnboardingStep,
    OnboardingWizard, SlackField, TelegramField, VoiceField, WizardMode, PROVIDERS,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Main color palette (matches existing OpenCrabs theme)
const BRAND_BLUE: Color = Color::Rgb(70, 130, 180);
const BRAND_GOLD: Color = Color::Rgb(218, 165, 32);
const ACCENT_GOLD: Color = Color::Rgb(184, 134, 11);

/// Render the entire onboarding wizard
pub fn render_onboarding(f: &mut Frame, wizard: &OnboardingWizard) {
    let area = f.area();

    // Center the wizard content
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(28.min(area.height.saturating_sub(2))),
            Constraint::Min(0),
        ])
        .split(area);

    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(60.min(area.width.saturating_sub(2))),
            Constraint::Min(0),
        ])
        .split(v_chunks[1]);

    let wizard_area = h_chunks[1];

    // Build wizard content — use 'static lines by converting all wizard data to owned strings
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Header
    let step = wizard.step;
    if step != OnboardingStep::Complete {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            render_progress_dots(&step),
            Style::default().fg(BRAND_BLUE),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            step.title().to_string(),
            Style::default()
                .fg(BRAND_GOLD)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            step.subtitle().to_string(),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
    }

    // Step-specific content
    match step {
        OnboardingStep::ModeSelect => render_mode_select(&mut lines, wizard),
        OnboardingStep::ProviderAuth => render_provider_auth(&mut lines, wizard),
        OnboardingStep::MessagingSetup => render_messaging_setup(&mut lines, wizard),
        OnboardingStep::Workspace => render_workspace(&mut lines, wizard),
        OnboardingStep::Gateway => render_gateway(&mut lines, wizard),
        OnboardingStep::Channels => render_channels(&mut lines, wizard),
        OnboardingStep::TelegramSetup => render_telegram_setup(&mut lines, wizard),
        OnboardingStep::DiscordSetup => render_discord_setup(&mut lines, wizard),
        OnboardingStep::SlackSetup => render_slack_setup(&mut lines, wizard),
        OnboardingStep::VoiceSetup => render_voice_setup(&mut lines, wizard),
        OnboardingStep::Daemon => render_daemon(&mut lines, wizard),
        OnboardingStep::HealthCheck => render_health_check(&mut lines, wizard),
        OnboardingStep::BrainSetup => render_brain_setup(&mut lines, wizard),
        OnboardingStep::Complete => render_complete(&mut lines, wizard),
    }

    // Error message
    if let Some(ref err) = wizard.error_message {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  ! {}", err),
            Style::default().fg(Color::Red),
        )));
    }

    // Navigation footer
    if step != OnboardingStep::Complete {
        lines.push(Line::from(""));
        let mut footer: Vec<Span<'static>> = vec![
            Span::styled(
                " [Esc] ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled("Back  ", Style::default().fg(Color::White)),
        ];

        if step != OnboardingStep::ModeSelect {
            footer.push(Span::styled(
                "[Tab] ",
                Style::default()
                    .fg(BRAND_BLUE)
                    .add_modifier(Modifier::BOLD),
            ));
            footer.push(Span::styled(
                "Next Field  ",
                Style::default().fg(Color::White),
            ));
        }

        footer.push(Span::styled(
            "[Enter] ",
            Style::default()
                .fg(ACCENT_GOLD)
                .add_modifier(Modifier::BOLD),
        ));
        footer.push(Span::styled("Confirm", Style::default().fg(Color::White)));

        lines.push(Line::from(footer));
    }

    let title_string = if step == OnboardingStep::Complete {
        " OpenCrabs Setup Complete ".to_string()
    } else {
        format!(
            " OpenCrabs Setup ({}/{}) ",
            step.number(),
            OnboardingStep::total()
        )
    };

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRAND_BLUE))
                .title(Span::styled(
                    title_string,
                    Style::default()
                        .fg(BRAND_BLUE)
                        .add_modifier(Modifier::BOLD),
                )),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, wizard_area);
}

/// Render progress dots (filled for completed, hollow for remaining)
fn render_progress_dots(step: &OnboardingStep) -> String {
    let current = step.number();
    let total = OnboardingStep::total();
    (1..=total)
        .map(|i| if i <= current { "●" } else { "○" })
        .collect::<Vec<_>>()
        .join(" ")
}

// --- Individual step renderers ---
// All functions produce Vec<Line<'static>> by using owned strings throughout.

fn render_mode_select(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    let qs_selected = wizard.mode == WizardMode::QuickStart;

    lines.push(Line::from(vec![
        Span::styled(
            if qs_selected { " > " } else { "   " },
            Style::default().fg(ACCENT_GOLD),
        ),
        Span::styled(
            if qs_selected { "[*]" } else { "[ ]" },
            Style::default().fg(if qs_selected { BRAND_GOLD } else { Color::DarkGray }),
        ),
        Span::styled(
            " QuickStart",
            Style::default()
                .fg(if qs_selected { Color::White } else { Color::DarkGray })
                .add_modifier(if qs_selected { Modifier::BOLD } else { Modifier::empty() }),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "       Sensible defaults, 4 steps",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    let adv_selected = !qs_selected;
    lines.push(Line::from(vec![
        Span::styled(
            if adv_selected { " > " } else { "   " },
            Style::default().fg(ACCENT_GOLD),
        ),
        Span::styled(
            if adv_selected { "[*]" } else { "[ ]" },
            Style::default().fg(if adv_selected { BRAND_GOLD } else { Color::DarkGray }),
        ),
        Span::styled(
            " Advanced",
            Style::default()
                .fg(if adv_selected { Color::White } else { Color::DarkGray })
                .add_modifier(if adv_selected { Modifier::BOLD } else { Modifier::empty() }),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "       Full control, all 7 steps",
        Style::default().fg(Color::DarkGray),
    )));
}

fn render_provider_auth(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    let is_custom = wizard.is_custom_provider();

    // Provider list
    for (i, provider) in PROVIDERS.iter().enumerate() {
        let selected = i == wizard.selected_provider;
        let focused = wizard.auth_field == AuthField::Provider;

        let prefix = if selected && focused { " > " } else { "   " };
        let marker = if selected { "[*]" } else { "[ ]" };

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(ACCENT_GOLD)),
            Span::styled(
                marker,
                Style::default().fg(if selected { BRAND_GOLD } else { Color::DarkGray }),
            ),
            Span::styled(
                format!(" {}", provider.name),
                Style::default()
                    .fg(if selected { Color::White } else { Color::DarkGray })
                    .add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() }),
            ),
        ]));
    }

    lines.push(Line::from(""));

    if is_custom {
        let base_focused = wizard.auth_field == AuthField::CustomBaseUrl;
        let model_focused = wizard.auth_field == AuthField::CustomModel;

        let base_display = if wizard.custom_base_url.is_empty() {
            "http://localhost:8000/v1".to_string()
        } else {
            wizard.custom_base_url.clone()
        };
        let cursor = if base_focused { "█" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(
                "  Base URL: ",
                Style::default().fg(if base_focused { BRAND_BLUE } else { Color::DarkGray }),
            ),
            Span::styled(
                format!("{}{}", base_display, cursor),
                Style::default().fg(if base_focused { Color::White } else { Color::DarkGray }),
            ),
        ]));

        let model_display = if wizard.custom_model.is_empty() {
            "model-name".to_string()
        } else {
            wizard.custom_model.clone()
        };
        let cursor = if model_focused { "█" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(
                "  Model:    ",
                Style::default().fg(if model_focused { BRAND_BLUE } else { Color::DarkGray }),
            ),
            Span::styled(
                format!("{}{}", model_display, cursor),
                Style::default().fg(if model_focused { Color::White } else { Color::DarkGray }),
            ),
        ]));
    } else {
        // Show help text for selected provider
        let provider = wizard.current_provider();
        for help_line in provider.help_lines {
            lines.push(Line::from(Span::styled(
                format!("  {}", help_line),
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )));
        }
        lines.push(Line::from(""));

        let key_focused = wizard.auth_field == AuthField::ApiKey;
        let key_label = provider.key_label;
        let (masked_key, key_hint) = if wizard.has_existing_key() {
            ("**************************".to_string(), " (already configured, type to replace)".to_string())
        } else if wizard.api_key_input.is_empty() {
            (format!("enter your {}", key_label.to_lowercase()), String::new())
        } else {
            ("*".repeat(wizard.api_key_input.len().min(30)), String::new())
        };
        let cursor = if key_focused && !wizard.has_existing_key() { "█" } else { "" };

        lines.push(Line::from(vec![
            Span::styled(
                format!("  {}: ", key_label),
                Style::default().fg(if key_focused { BRAND_BLUE } else { Color::DarkGray }),
            ),
            Span::styled(
                format!("{}{}", masked_key, cursor),
                Style::default().fg(if wizard.has_existing_key() { Color::Green } else if key_focused { Color::White } else { Color::DarkGray }),
            ),
        ]));

        if !key_hint.is_empty() && key_focused {
            lines.push(Line::from(Span::styled(
                format!("  {}", key_hint.trim()),
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )));
        }

        // Model selection
        let model_focused = wizard.auth_field == AuthField::Model;
        let models = wizard.current_provider().models;
        if !models.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Model:".to_string(),
                Style::default().fg(if model_focused { BRAND_BLUE } else { Color::DarkGray }),
            )));

            for (i, model) in models.iter().enumerate() {
                let selected = i == wizard.selected_model;
                let prefix = if selected && model_focused { " > " } else { "   " };
                let marker = if selected { "(*)" } else { "( )" };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {}{} ", prefix, marker),
                        Style::default().fg(if selected { ACCENT_GOLD } else { Color::DarkGray }),
                    ),
                    Span::styled(
                        model.to_string(),
                        Style::default().fg(if selected { Color::White } else { Color::DarkGray }),
                    ),
                ]));
            }
        }
    }
}

fn render_messaging_setup(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    lines.push(Line::from(Span::styled(
        "  Pick your channels (Space to toggle):",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    // Channel entries: (field, label, enabled, description)
    let channels: Vec<(MessagingField, &str, bool, &str)> = vec![
        (MessagingField::Telegram, "Telegram", wizard.messaging_telegram, "Bot token (via @BotFather)"),
        (MessagingField::Discord, "Discord", wizard.messaging_discord, "Bot token (via Developer Portal)"),
        (MessagingField::WhatsApp, "WhatsApp", wizard.messaging_whatsapp, "QR code pairing — set up later"),
        (MessagingField::Slack, "Slack", wizard.messaging_slack, "Socket Mode (bot + app tokens)"),
    ];

    for (field, label, on, desc) in &channels {
        let focused = wizard.messaging_field == *field;
        lines.push(Line::from(vec![
            Span::styled(
                if focused { " > " } else { "   " },
                Style::default().fg(ACCENT_GOLD),
            ),
            Span::styled(
                if *on { "[x]" } else { "[ ]" },
                Style::default().fg(if *on { BRAND_GOLD } else { Color::DarkGray }),
            ),
            Span::styled(
                format!(" {}", label),
                Style::default()
                    .fg(if focused { Color::White } else { Color::DarkGray })
                    .add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            format!("       {}", desc),
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Skip all with Enter if you're not into it",
        Style::default().fg(Color::DarkGray),
    )));
}

fn render_workspace(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    let path_focused = wizard.focused_field == 0;
    let seed_focused = wizard.focused_field == 1;

    let cursor = if path_focused { "█" } else { "" };
    lines.push(Line::from(vec![
        Span::styled(
            "  Path: ",
            Style::default().fg(if path_focused { BRAND_BLUE } else { Color::DarkGray }),
        ),
        Span::styled(
            format!("{}{}", wizard.workspace_path, cursor),
            Style::default().fg(if path_focused { Color::White } else { Color::DarkGray }),
        ),
    ]));
    lines.push(Line::from(""));

    lines.push(Line::from(vec![
        Span::styled(
            if seed_focused { " > " } else { "   " },
            Style::default().fg(ACCENT_GOLD),
        ),
        Span::styled(
            if wizard.seed_templates { "[x]" } else { "[ ]" },
            Style::default().fg(if wizard.seed_templates { BRAND_GOLD } else { Color::DarkGray }),
        ),
        Span::styled(
            " Seed template files",
            Style::default().fg(if seed_focused { Color::White } else { Color::DarkGray }),
        ),
    ]));

    lines.push(Line::from(Span::styled(
        "       SOUL.md, IDENTITY.md, USER.md, ...",
        Style::default().fg(Color::DarkGray),
    )));
}

fn render_gateway(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    let port_focused = wizard.focused_field == 0;
    let bind_focused = wizard.focused_field == 1;
    let auth_focused = wizard.focused_field == 2;

    let cursor_p = if port_focused { "█" } else { "" };
    lines.push(Line::from(vec![
        Span::styled(
            "  Port: ",
            Style::default().fg(if port_focused { BRAND_BLUE } else { Color::DarkGray }),
        ),
        Span::styled(
            format!("{}{}", wizard.gateway_port, cursor_p),
            Style::default().fg(if port_focused { Color::White } else { Color::DarkGray }),
        ),
    ]));

    let cursor_b = if bind_focused { "█" } else { "" };
    lines.push(Line::from(vec![
        Span::styled(
            "  Bind: ",
            Style::default().fg(if bind_focused { BRAND_BLUE } else { Color::DarkGray }),
        ),
        Span::styled(
            format!("{}{}", wizard.gateway_bind, cursor_b),
            Style::default().fg(if bind_focused { Color::White } else { Color::DarkGray }),
        ),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Auth Mode:".to_string(),
        Style::default().fg(if auth_focused { BRAND_BLUE } else { Color::DarkGray }),
    )));

    let token_selected = wizard.gateway_auth == 0;
    lines.push(Line::from(vec![
        Span::styled(
            if token_selected && auth_focused { "  > " } else { "    " },
            Style::default().fg(ACCENT_GOLD),
        ),
        Span::styled(
            if token_selected { "(*)" } else { "( )" },
            Style::default().fg(if token_selected { BRAND_GOLD } else { Color::DarkGray }),
        ),
        Span::styled(
            " Token (auto-generated)",
            Style::default().fg(if token_selected { Color::White } else { Color::DarkGray }),
        ),
    ]));

    let none_selected = !token_selected;
    lines.push(Line::from(vec![
        Span::styled(
            if none_selected && auth_focused { "  > " } else { "    " },
            Style::default().fg(ACCENT_GOLD),
        ),
        Span::styled(
            if none_selected { "(*)" } else { "( )" },
            Style::default().fg(if none_selected { BRAND_GOLD } else { Color::DarkGray }),
        ),
        Span::styled(
            " None (open access)",
            Style::default().fg(if none_selected { Color::White } else { Color::DarkGray }),
        ),
    ]));
}

fn render_channels(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    lines.push(Line::from(Span::styled(
        "  Toggle channels with Space:",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    for (i, (name, enabled)) in wizard.channel_toggles.iter().enumerate() {
        let focused = i == wizard.focused_field;
        let prefix = if focused { " > " } else { "   " };
        let marker = if *enabled { "[x]" } else { "[ ]" };

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(ACCENT_GOLD)),
            Span::styled(
                marker,
                Style::default().fg(if *enabled { BRAND_GOLD } else { Color::DarkGray }),
            ),
            Span::styled(
                format!(" {}", name),
                Style::default()
                    .fg(if focused { Color::White } else { Color::DarkGray })
                    .add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }),
            ),
        ]));
    }
}

fn render_telegram_setup(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    // Help text
    lines.push(Line::from(Span::styled(
        "  1. Open Telegram, search @BotFather",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(Span::styled(
        "  2. Send /newbot, follow the prompts",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(Span::styled(
        "  3. Copy the bot token and paste below",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(""));

    // Bot token input
    let token_focused = wizard.telegram_field == TelegramField::BotToken;
    let (masked_token, token_hint) = if wizard.has_existing_telegram_token() {
        ("**************************".to_string(), " (already configured)".to_string())
    } else if wizard.telegram_token_input.is_empty() {
        ("paste your bot token".to_string(), String::new())
    } else {
        ("*".repeat(wizard.telegram_token_input.len().min(30)), String::new())
    };
    let cursor = if token_focused && !wizard.has_existing_telegram_token() { "█" } else { "" };

    lines.push(Line::from(vec![
        Span::styled(
            "  Bot Token: ",
            Style::default().fg(if token_focused { BRAND_BLUE } else { Color::DarkGray }),
        ),
        Span::styled(
            format!("{}{}", masked_token, cursor),
            Style::default().fg(
                if wizard.has_existing_telegram_token() { Color::Green }
                else if token_focused { Color::White }
                else { Color::DarkGray }
            ),
        ),
    ]));

    if !token_hint.is_empty() && token_focused {
        lines.push(Line::from(Span::styled(
            format!("  {}", token_hint.trim()),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  After setup, send /start to your bot",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(Span::styled(
        "  to get your user ID for the allowlist",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Skip with Enter if you'll add it later",
        Style::default().fg(Color::DarkGray),
    )));
}

fn render_discord_setup(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    // Help text
    lines.push(Line::from(Span::styled(
        "  1. Go to discord.com/developers/applications",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(Span::styled(
        "  2. Create app > Bot > Copy token",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(Span::styled(
        "  3. Enable Message Content Intent",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(""));

    // Bot token input
    let token_focused = wizard.discord_field == DiscordField::BotToken;
    let (masked_token, token_hint) = if wizard.has_existing_discord_token() {
        ("**************************".to_string(), " (already configured)".to_string())
    } else if wizard.discord_token_input.is_empty() {
        ("paste your bot token".to_string(), String::new())
    } else {
        ("*".repeat(wizard.discord_token_input.len().min(30)), String::new())
    };
    let cursor = if token_focused && !wizard.has_existing_discord_token() { "\u{2588}" } else { "" };

    lines.push(Line::from(vec![
        Span::styled(
            "  Bot Token: ",
            Style::default().fg(if token_focused { BRAND_BLUE } else { Color::DarkGray }),
        ),
        Span::styled(
            format!("{}{}", masked_token, cursor),
            Style::default().fg(
                if wizard.has_existing_discord_token() { Color::Green }
                else if token_focused { Color::White }
                else { Color::DarkGray }
            ),
        ),
    ]));

    if !token_hint.is_empty() && token_focused {
        lines.push(Line::from(Span::styled(
            format!("  {}", token_hint.trim()),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Skip with Enter if you'll add it later",
        Style::default().fg(Color::DarkGray),
    )));
}

fn render_slack_setup(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    // Help text
    lines.push(Line::from(Span::styled(
        "  1. Go to api.slack.com/apps > Create App",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(Span::styled(
        "  2. Enable Socket Mode > copy App Token",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(Span::styled(
        "  3. OAuth > Install > copy Bot Token",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(""));

    // Bot token input
    let bot_focused = wizard.slack_field == SlackField::BotToken;
    let (masked_bot, bot_hint) = if wizard.has_existing_slack_bot_token() {
        ("**************************".to_string(), " (already configured)".to_string())
    } else if wizard.slack_bot_token_input.is_empty() {
        ("xoxb-...".to_string(), String::new())
    } else {
        ("*".repeat(wizard.slack_bot_token_input.len().min(30)), String::new())
    };
    let cursor_b = if bot_focused && !wizard.has_existing_slack_bot_token() { "\u{2588}" } else { "" };

    lines.push(Line::from(vec![
        Span::styled(
            "  Bot Token: ",
            Style::default().fg(if bot_focused { BRAND_BLUE } else { Color::DarkGray }),
        ),
        Span::styled(
            format!("{}{}", masked_bot, cursor_b),
            Style::default().fg(
                if wizard.has_existing_slack_bot_token() { Color::Green }
                else if bot_focused { Color::White }
                else { Color::DarkGray }
            ),
        ),
    ]));

    if !bot_hint.is_empty() && bot_focused {
        lines.push(Line::from(Span::styled(
            format!("  {}", bot_hint.trim()),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
    }

    // App token input
    let app_focused = wizard.slack_field == SlackField::AppToken;
    let (masked_app, app_hint) = if wizard.has_existing_slack_app_token() {
        ("**************************".to_string(), " (already configured)".to_string())
    } else if wizard.slack_app_token_input.is_empty() {
        ("xapp-...".to_string(), String::new())
    } else {
        ("*".repeat(wizard.slack_app_token_input.len().min(30)), String::new())
    };
    let cursor_a = if app_focused && !wizard.has_existing_slack_app_token() { "\u{2588}" } else { "" };

    lines.push(Line::from(vec![
        Span::styled(
            "  App Token: ",
            Style::default().fg(if app_focused { BRAND_BLUE } else { Color::DarkGray }),
        ),
        Span::styled(
            format!("{}{}", masked_app, cursor_a),
            Style::default().fg(
                if wizard.has_existing_slack_app_token() { Color::Green }
                else if app_focused { Color::White }
                else { Color::DarkGray }
            ),
        ),
    ]));

    if !app_hint.is_empty() && app_focused {
        lines.push(Line::from(Span::styled(
            format!("  {}", app_hint.trim()),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Skip with Enter if you'll add tokens later",
        Style::default().fg(Color::DarkGray),
    )));
}

fn render_voice_setup(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    // STT section
    lines.push(Line::from(Span::styled(
        "  Speech-to-Text (Groq Whisper)".to_string(),
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "  Transcribes voice notes from Telegram",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(""));

    let groq_focused = wizard.voice_field == VoiceField::GroqApiKey;
    let (masked_key, key_hint) = if wizard.has_existing_groq_key() {
        ("**************************".to_string(), " (from GROQ_API_KEY env)".to_string())
    } else if wizard.groq_api_key_input.is_empty() {
        ("get key from console.groq.com".to_string(), String::new())
    } else {
        ("*".repeat(wizard.groq_api_key_input.len().min(30)), String::new())
    };
    let cursor = if groq_focused && !wizard.has_existing_groq_key() { "█" } else { "" };

    lines.push(Line::from(vec![
        Span::styled(
            "  Groq Key: ",
            Style::default().fg(if groq_focused { BRAND_BLUE } else { Color::DarkGray }),
        ),
        Span::styled(
            format!("{}{}", masked_key, cursor),
            Style::default().fg(
                if wizard.has_existing_groq_key() { Color::Green }
                else if groq_focused { Color::White }
                else { Color::DarkGray }
            ),
        ),
    ]));

    if !key_hint.is_empty() && groq_focused {
        lines.push(Line::from(Span::styled(
            format!("  {}", key_hint.trim()),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
    }

    lines.push(Line::from(""));

    // TTS section
    lines.push(Line::from(Span::styled(
        "  Text-to-Speech (OpenAI TTS)".to_string(),
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "  Reply with voice notes (uses OpenAI key)",
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(""));

    let tts_focused = wizard.voice_field == VoiceField::TtsToggle;
    lines.push(Line::from(vec![
        Span::styled(
            if tts_focused { " > " } else { "   " },
            Style::default().fg(ACCENT_GOLD),
        ),
        Span::styled(
            if wizard.tts_enabled { "[x]" } else { "[ ]" },
            Style::default().fg(if wizard.tts_enabled { BRAND_GOLD } else { Color::DarkGray }),
        ),
        Span::styled(
            " Enable TTS replies (ash voice)",
            Style::default()
                .fg(if tts_focused { Color::White } else { Color::DarkGray })
                .add_modifier(if tts_focused { Modifier::BOLD } else { Modifier::empty() }),
        ),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Skip with Enter to set up later",
        Style::default().fg(Color::DarkGray),
    )));
}

fn render_daemon(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    let platform = if cfg!(target_os = "linux") {
        "systemd user unit"
    } else if cfg!(target_os = "macos") {
        "LaunchAgent"
    } else {
        "background service"
    };

    lines.push(Line::from(Span::styled(
        format!("  Install as {} ?", platform),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    let yes_selected = wizard.install_daemon;
    lines.push(Line::from(vec![
        Span::styled(
            if yes_selected { " > " } else { "   " },
            Style::default().fg(ACCENT_GOLD),
        ),
        Span::styled(
            if yes_selected { "(*)" } else { "( )" },
            Style::default().fg(if yes_selected { BRAND_GOLD } else { Color::DarkGray }),
        ),
        Span::styled(
            " Yes, install daemon",
            Style::default().fg(if yes_selected { Color::White } else { Color::DarkGray }),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::styled(
            if !yes_selected { " > " } else { "   " },
            Style::default().fg(ACCENT_GOLD),
        ),
        Span::styled(
            if !yes_selected { "(*)" } else { "( )" },
            Style::default().fg(if !yes_selected { BRAND_GOLD } else { Color::DarkGray }),
        ),
        Span::styled(
            " Skip for now",
            Style::default().fg(if !yes_selected { Color::White } else { Color::DarkGray }),
        ),
    ]));
}

fn render_health_check(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    for (name, status) in &wizard.health_results {
        let (icon, color) = match status {
            HealthStatus::Pending => ("...", Color::DarkGray),
            HealthStatus::Running => ("...", ACCENT_GOLD),
            HealthStatus::Pass => ("OK", Color::Green),
            HealthStatus::Fail(_) => ("FAIL", Color::Red),
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("  [{:<4}] ", icon),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(name.clone(), Style::default().fg(Color::White)),
        ]));

        if let HealthStatus::Fail(reason) = status {
            lines.push(Line::from(Span::styled(
                format!("          {}", reason),
                Style::default().fg(Color::Red),
            )));
        }
    }

    lines.push(Line::from(""));

    if wizard.health_complete {
        if wizard.all_health_passed() {
            lines.push(Line::from(Span::styled(
                "  All checks passed!".to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "  Press Enter to finish setup".to_string(),
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "  Some checks failed.".to_string(),
                Style::default().fg(Color::Red),
            )));
            lines.push(Line::from(vec![
                Span::styled(
                    "  [R] ",
                    Style::default()
                        .fg(BRAND_BLUE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Re-run  ", Style::default().fg(Color::White)),
                Span::styled(
                    "[Esc] ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled("Go back and fix", Style::default().fg(Color::White)),
            ]));
        }
    }
}

fn render_brain_setup(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    // Show generating state
    if wizard.brain_generating {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Cooking up your brain files...".to_string(),
            Style::default()
                .fg(ACCENT_GOLD)
                .add_modifier(Modifier::BOLD | Modifier::ITALIC),
        )));
        lines.push(Line::from(Span::styled(
            "  Your agent is getting to know you".to_string(),
            Style::default().fg(Color::DarkGray),
        )));
        return;
    }

    // Show success state
    if wizard.brain_generated {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Brain files locked in!".to_string(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  Your agent knows the deal now".to_string(),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Press Enter to finish setup".to_string(),
            Style::default().fg(Color::DarkGray),
        )));
        return;
    }

    // Show error state (with fallback notice)
    if let Some(ref err) = wizard.brain_error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {} — rolling with defaults", err),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Press Enter to continue".to_string(),
            Style::default().fg(Color::DarkGray),
        )));
        return;
    }

    // "About You" text area
    let me_focused = wizard.brain_field == BrainField::AboutMe;
    lines.push(Line::from(Span::styled(
        "  About You:".to_string(),
        Style::default()
            .fg(if me_focused { BRAND_BLUE } else { Color::DarkGray })
            .add_modifier(Modifier::BOLD),
    )));

    let me_display = if wizard.about_me.is_empty() && !me_focused {
        "  name, role, links, projects, whatever you got".to_string()
    } else {
        let cursor = if me_focused { "█" } else { "" };
        format!("  {}{}", wizard.about_me, cursor)
    };
    let me_style = if wizard.about_me.is_empty() && !me_focused {
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
    } else {
        Style::default().fg(if me_focused { Color::White } else { Color::DarkGray })
    };
    // Wrap long text into multiple lines
    for chunk in wrap_text(&me_display, 54) {
        lines.push(Line::from(Span::styled(chunk, me_style)));
    }

    lines.push(Line::from(""));

    // "Your Agent" text area
    let agent_focused = wizard.brain_field == BrainField::AboutAgent;
    lines.push(Line::from(Span::styled(
        "  Your Agent:".to_string(),
        Style::default()
            .fg(if agent_focused { BRAND_BLUE } else { Color::DarkGray })
            .add_modifier(Modifier::BOLD),
    )));

    let agent_display = if wizard.about_agent.is_empty() && !agent_focused {
        "  personality, vibe, how it should talk to you".to_string()
    } else {
        let cursor = if agent_focused { "█" } else { "" };
        format!("  {}{}", wizard.about_agent, cursor)
    };
    let agent_style = if wizard.about_agent.is_empty() && !agent_focused {
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
    } else {
        Style::default().fg(if agent_focused { Color::White } else { Color::DarkGray })
    };
    for chunk in wrap_text(&agent_display, 54) {
        lines.push(Line::from(Span::styled(chunk, agent_style)));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  The more you drop the better it covers your ass".to_string(),
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
    )));
}

/// Wrap a string into chunks of max_width display columns
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthStr;
    if text.width() <= max_width {
        return vec![text.to_string()];
    }
    let mut result = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if remaining.width() <= max_width {
            result.push(remaining.to_string());
            break;
        }
        // Find byte index at display width limit
        let byte_limit = super::render::char_boundary_at_width(remaining, max_width);
        // Try to break at a space
        let break_at = remaining[..byte_limit]
            .rfind(' ')
            .unwrap_or(byte_limit);
        let break_at = if break_at == 0 { byte_limit.max(remaining.ceil_char_boundary(1)) } else { break_at };
        result.push(remaining[..break_at].to_string());
        remaining = remaining[break_at..].trim_start();
    }
    result
}

fn render_complete(lines: &mut Vec<Line<'static>>, wizard: &OnboardingWizard) {
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Setup complete!".to_string(),
        Style::default()
            .fg(BRAND_GOLD)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Summary
    let provider = &PROVIDERS[wizard.selected_provider];
    lines.push(Line::from(vec![
        Span::styled("  Provider: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            provider.name.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    if wizard.is_custom_provider() {
        lines.push(Line::from(vec![
            Span::styled("  Base URL: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                wizard.custom_base_url.clone(),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Model:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                wizard.custom_model.clone(),
                Style::default().fg(Color::White),
            ),
        ]));
    } else {
        let model = provider
            .models
            .get(wizard.selected_model)
            .unwrap_or(&"default");
        lines.push(Line::from(vec![
            Span::styled("  Model:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                model.to_string(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("  Workspace:", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(" {}", wizard.workspace_path),
            Style::default().fg(Color::White),
        ),
    ]));

    if wizard.mode == WizardMode::Advanced {
        lines.push(Line::from(vec![
            Span::styled("  Gateway:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}:{}", wizard.gateway_bind, wizard.gateway_port),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Entering OpenCrabs...".to_string(),
        Style::default()
            .fg(ACCENT_GOLD)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC),
    )));
}
