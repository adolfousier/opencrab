//! Onboarding Wizard
//!
//! A 7-step TUI-based onboarding wizard for first-time OpenCrabs users.
//! Handles mode selection, provider/auth setup, workspace, gateway,
//! channels, daemon installation, and health check.

mod brain;
mod channels;
mod config;
mod fetch;
pub(crate) mod helpers;
mod input;
mod keys;
mod models;
mod navigation;
mod types;
pub mod voice;
mod wizard;

// Re-export all public types
pub use types::{
    AuthField, BrainField, CHANNEL_NAMES, ChannelTestStatus, CodexDeviceFlowStatus, DiscordField,
    EXISTING_KEY_SENTINEL, GitHubDeviceFlowStatus, HealthStatus, ImageField, OnboardingStep,
    PROVIDERS, ProviderInfo, SlackField, SttProvider, TEMPLATE_FILES, TelegramField, TrelloField,
    TtsProvider, VoiceField, WhatsAppField, WizardAction, WizardMode,
};

pub use wizard::OnboardingWizard;

pub(crate) use brain::parse_brain_sections;
pub use fetch::{fetch_provider_models, is_first_time};

/// Welcome message sent once after the user completes first-time onboarding.
pub const WELCOME_MESSAGE: &str = "Holy shit, we are live. Onboard complete, brain files locked and loaded. I am ready to help you out. What about we start with a cronjob or heartbeat so I can reach out randomly, check missing tasks, or just bug you when something needs attention?";
