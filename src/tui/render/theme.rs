//! Theme core: semantic color roles + runtime theme resolution.
//!
//! S2 of the theme system (#1364). Render sites resolve colors through
//! [`role`] instead of reading palette consts directly, so `/theme` and
//! `[tui.theme]` can switch the whole UI at runtime. The default
//! `crab-dark` theme is byte-identical to the post-S1 palette by
//! construction: every field IS the palette const.
//!
//! Granularity note: one Role per palette const. Collapsing roles would
//! change crab-dark rendering and break the #1363 byte-identical
//! contract; presets that don't distinguish two roles simply map them
//! to the same upstream hex.
//!
//! Performance: `role()` takes an uncontended RwLock read per call and
//! ratatui calls it a few hundred times per frame, nanoseconds each.
//! Revisit only if a profile says so.
//!
//! Decorative cycles (project badge rotation in `palette`) are
//! deliberately NOT roles: they are preset-agnostic by design.

use std::sync::RwLock;

use ratatui::style::Color;

use super::palette;

/// Semantic color roles, one per themeable palette const.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Accent,         // palette::ORANGE
    AccentTeal,     // palette::TEAL (ANSI Cyan)
    AccentSoft,     // palette::WHITE
    TextPrimary,    // palette::TEXT_PRIMARY
    TextSecondary,  // palette::TEXT_SECONDARY
    TextMuted,      // palette::TEXT_MUTED
    TextDim,        // palette::TEXT_DIM
    Gray,           // palette::GRAY
    GrayMid,        // palette::GRAY_MID
    GrayDetail,     // palette::GRAY_DETAIL
    GrayDim,        // palette::GRAY_DIM
    GrayDark,       // palette::GRAY_DARK
    GrayBase,       // palette::GRAY_BASE
    GrayLight,      // palette::GRAY_LIGHT
    GraySoft,       // palette::GRAY_SOFT
    GrayMuted,      // palette::GRAY_MUTED
    Success,        // palette::SUCCESS
    AnalyticsGreen, // palette::ANALYTICS_GREEN
    GreenCheck,     // palette::GREEN_CHECK
    Error,          // palette::ERROR
    ErrorSoft,      // palette::ERROR_SOFT
    ErrorFaded,     // palette::ERROR_FADED
    Warning,        // palette::WARNING
    WarningMuted,   // palette::WARNING_MUTED
    AmberMuted,     // palette::AMBER_MUTED
    TealVivid,      // palette::TEAL_VIVID
    TealBright,     // palette::TEAL_BRIGHT
    TealMuted,      // palette::TEAL_MUTED
    TealCalm,       // palette::TEAL_CALM
    BlueSlate,      // palette::BLUE_SLATE
    BlueSteel,      // palette::BLUE_STEEL
    BlueLink,       // palette::BLUE_LINK
    BlueSky,        // palette::BLUE_SKY
    BlueSoft,       // palette::BLUE_SOFT
    BlueVivid,      // palette::BLUE_VIVID
    BlueCode,       // palette::BLUE_CODE
    SelectionBg,    // palette::SELECTION_BG
    SurfacePanel,   // palette::SURFACE_PANEL
    SurfaceQr,      // palette::SURFACE_QR
    SurfaceCode,    // palette::SURFACE_CODE
    SurfaceCodeAlt, // palette::SURFACE_CODE_ALT
    Ink,            // palette::INK
    PurpleSoft,     // palette::PURPLE_SOFT
}

/// A complete role-to-color mapping for one theme.
#[derive(Debug, Clone)]
pub struct ThemeColors {
    pub accent: Color,
    pub accent_teal: Color,
    pub accent_soft: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub text_dim: Color,
    pub gray: Color,
    pub gray_mid: Color,
    pub gray_detail: Color,
    pub gray_dim: Color,
    pub gray_dark: Color,
    pub gray_base: Color,
    pub gray_light: Color,
    pub gray_soft: Color,
    pub gray_muted: Color,
    pub success: Color,
    pub analytics_green: Color,
    pub green_check: Color,
    pub error: Color,
    pub error_soft: Color,
    pub error_faded: Color,
    pub warning: Color,
    pub warning_muted: Color,
    pub amber_muted: Color,
    pub teal_vivid: Color,
    pub teal_bright: Color,
    pub teal_muted: Color,
    pub teal_calm: Color,
    pub blue_slate: Color,
    pub blue_steel: Color,
    pub blue_link: Color,
    pub blue_sky: Color,
    pub blue_soft: Color,
    pub blue_vivid: Color,
    pub blue_code: Color,
    pub selection_bg: Color,
    pub surface_panel: Color,
    pub surface_qr: Color,
    pub surface_code: Color,
    pub surface_code_alt: Color,
    pub ink: Color,
    pub purple_soft: Color,
}

impl ThemeColors {
    /// Resolve one role. Panics never: every role has a field.
    pub fn get(&self, role: Role) -> Color {
        match role {
            Role::Accent => self.accent,
            Role::AccentTeal => self.accent_teal,
            Role::AccentSoft => self.accent_soft,
            Role::TextPrimary => self.text_primary,
            Role::TextSecondary => self.text_secondary,
            Role::TextMuted => self.text_muted,
            Role::TextDim => self.text_dim,
            Role::Gray => self.gray,
            Role::GrayMid => self.gray_mid,
            Role::GrayDetail => self.gray_detail,
            Role::GrayDim => self.gray_dim,
            Role::GrayDark => self.gray_dark,
            Role::GrayBase => self.gray_base,
            Role::GrayLight => self.gray_light,
            Role::GraySoft => self.gray_soft,
            Role::GrayMuted => self.gray_muted,
            Role::Success => self.success,
            Role::AnalyticsGreen => self.analytics_green,
            Role::GreenCheck => self.green_check,
            Role::Error => self.error,
            Role::ErrorSoft => self.error_soft,
            Role::ErrorFaded => self.error_faded,
            Role::Warning => self.warning,
            Role::WarningMuted => self.warning_muted,
            Role::AmberMuted => self.amber_muted,
            Role::TealVivid => self.teal_vivid,
            Role::TealBright => self.teal_bright,
            Role::TealMuted => self.teal_muted,
            Role::TealCalm => self.teal_calm,
            Role::BlueSlate => self.blue_slate,
            Role::BlueSteel => self.blue_steel,
            Role::BlueLink => self.blue_link,
            Role::BlueSky => self.blue_sky,
            Role::BlueSoft => self.blue_soft,
            Role::BlueVivid => self.blue_vivid,
            Role::BlueCode => self.blue_code,
            Role::SelectionBg => self.selection_bg,
            Role::SurfacePanel => self.surface_panel,
            Role::SurfaceQr => self.surface_qr,
            Role::SurfaceCode => self.surface_code,
            Role::SurfaceCodeAlt => self.surface_code_alt,
            Role::Ink => self.ink,
            Role::PurpleSoft => self.purple_soft,
        }
    }
}

/// A named theme. `crab_dark` below is the default and must stay
/// byte-identical to the S1 palette (regression-tested in `presets`).
pub struct Theme {
    pub name: &'static str,
    pub colors: ThemeColors,
}

/// The default theme: every field IS the post-S1 palette const, so
/// byte-identical rendering is guaranteed at compile time.
pub static CRAB_DARK: Theme = Theme {
    name: "crab-dark",
    colors: ThemeColors {
        accent: palette::ORANGE,
        accent_teal: palette::TEAL,
        accent_soft: palette::WHITE,
        text_primary: palette::TEXT_PRIMARY,
        text_secondary: palette::TEXT_SECONDARY,
        text_muted: palette::TEXT_MUTED,
        text_dim: palette::TEXT_DIM,
        gray: palette::GRAY,
        gray_mid: palette::GRAY_MID,
        gray_detail: palette::GRAY_DETAIL,
        gray_dim: palette::GRAY_DIM,
        gray_dark: palette::GRAY_DARK,
        gray_base: palette::GRAY_BASE,
        gray_light: palette::GRAY_LIGHT,
        gray_soft: palette::GRAY_SOFT,
        gray_muted: palette::GRAY_MUTED,
        success: palette::SUCCESS,
        analytics_green: palette::ANALYTICS_GREEN,
        green_check: palette::GREEN_CHECK,
        error: palette::ERROR,
        error_soft: palette::ERROR_SOFT,
        error_faded: palette::ERROR_FADED,
        warning: palette::WARNING,
        warning_muted: palette::WARNING_MUTED,
        amber_muted: palette::AMBER_MUTED,
        teal_vivid: palette::TEAL_VIVID,
        teal_bright: palette::TEAL_BRIGHT,
        teal_muted: palette::TEAL_MUTED,
        teal_calm: palette::TEAL_CALM,
        blue_slate: palette::BLUE_SLATE,
        blue_steel: palette::BLUE_STEEL,
        blue_link: palette::BLUE_LINK,
        blue_sky: palette::BLUE_SKY,
        blue_soft: palette::BLUE_SOFT,
        blue_vivid: palette::BLUE_VIVID,
        blue_code: palette::BLUE_CODE,
        selection_bg: palette::SELECTION_BG,
        surface_panel: palette::SURFACE_PANEL,
        surface_qr: palette::SURFACE_QR,
        surface_code: palette::SURFACE_CODE,
        surface_code_alt: palette::SURFACE_CODE_ALT,
        ink: palette::INK,
        purple_soft: palette::PURPLE_SOFT,
    },
};

/// Active theme slot. `None` means default (crab-dark); avoids any
/// const-eval constraints on the lock's initializer.
static ACTIVE: RwLock<Option<&'static Theme>> = RwLock::new(None);

/// Currently active theme (crab-dark until [`set`] is called).
pub fn active() -> &'static Theme {
    ACTIVE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .unwrap_or(&CRAB_DARK)
}

/// Switch the active theme. Callers own validation (preset lookup by
/// name lives in `presets::by_name`).
pub fn set(theme: &'static Theme) {
    *ACTIVE.write().unwrap_or_else(|e| e.into_inner()) = Some(theme);
}

/// Return to the default theme.
pub fn reset() {
    *ACTIVE.write().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Resolve a role against the active theme. The one call render sites
/// make after the S2 codemod.
pub fn role(role: Role) -> Color {
    active().colors.get(role)
}
