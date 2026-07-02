//! Regression test for issue #271: `/onboard:channels <name>` must jump
//! straight to that channel's setup dialog, not fall back to the menu.
//!
//! The deep-link was broken because `handle_slash_command` extracted only the
//! first word into `cmd`, then stripped `/onboard` from `cmd` instead of the
//! full input. The channel argument (`whatsapp`, `telegram`, etc.) was silently
//! dropped and `open_channel_setup` never fired.
//!
//! These tests pin the `open_channel_setup` contract for every supported
//! channel name so the deep-link target can't silently regress.

use crate::tui::onboarding::{OnboardingStep, OnboardingWizard};

#[test]
fn open_channel_setup_whatsapp() {
    let mut w = OnboardingWizard::default();
    assert!(w.open_channel_setup("whatsapp"));
    assert_eq!(w.step, OnboardingStep::WhatsAppSetup);
}

#[test]
fn open_channel_setup_telegram() {
    let mut w = OnboardingWizard::default();
    assert!(w.open_channel_setup("telegram"));
    assert_eq!(w.step, OnboardingStep::TelegramSetup);
}

#[test]
fn open_channel_setup_discord() {
    let mut w = OnboardingWizard::default();
    assert!(w.open_channel_setup("discord"));
    assert_eq!(w.step, OnboardingStep::DiscordSetup);
}

#[test]
fn open_channel_setup_slack() {
    let mut w = OnboardingWizard::default();
    assert!(w.open_channel_setup("slack"));
    assert_eq!(w.step, OnboardingStep::SlackSetup);
}

#[test]
fn open_channel_setup_trello() {
    let mut w = OnboardingWizard::default();
    assert!(w.open_channel_setup("trello"));
    assert_eq!(w.step, OnboardingStep::TrelloSetup);
}

#[test]
fn open_channel_setup_unknown_returns_false() {
    let mut w = OnboardingWizard::default();
    // Unknown name must NOT transition — caller falls back to menu
    assert!(!w.open_channel_setup("irc"));
    assert_eq!(w.step, OnboardingStep::ModeSelect); // unchanged
}

#[test]
fn open_channel_setup_empty_returns_false() {
    let mut w = OnboardingWizard::default();
    assert!(!w.open_channel_setup(""));
    assert_eq!(w.step, OnboardingStep::ModeSelect);
}
