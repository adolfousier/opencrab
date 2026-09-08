//! A config that names providers must not grow one it never named.
//!
//! `ProviderConfigs::xiaomi` is the only provider field carrying a
//! `#[serde(default = "...")]` constructor; the other eighteen default to
//! `None`. The constructor does not fire when the `[providers]` table is
//! absent entirely (the field's own `#[serde(default)]` supplies a derived
//! `ProviderConfigs` first), so it takes a config that HAS providers to
//! trigger: every real config on disk.
//!
//! Materialising it is harmless while the config is only read. It stops being
//! harmless the moment the struct is written back, because TOML has no null
//! representation and serde drops the eighteen `None`s on the way out. Any
//! whole-file rewrite of a parsed config (`Config::save`, which serialises the
//! struct rather than merging into the document) turns "the providers I
//! configured" into "xiaomi, which I never chose", losing the rest.

use crate::config::Config;

/// A config naming one provider, as a first-run file would after the wizard
/// writes the section for the provider the user picked.
const ONE_PROVIDER: &str = "\
[providers.claude_cli]
enabled = true
default_model = \"opus-5\"
";

#[test]
fn a_named_provider_does_not_drag_in_an_unnamed_one() {
    let config: Config = toml::from_str(ONE_PROVIDER).expect("config must parse");

    assert!(
        config.providers.claude_cli.is_some(),
        "the provider the config actually names must survive the parse"
    );
    assert!(
        config.providers.xiaomi.is_none(),
        "parsing a config that names claude_cli materialised a xiaomi section \
         from its serde default constructor; nothing in the file asked for it"
    );
}

#[test]
fn round_tripping_a_config_keeps_the_providers_it_named() {
    let config: Config = toml::from_str(ONE_PROVIDER).expect("config must parse");
    let rendered = toml::to_string_pretty(&config).expect("config must serialise");

    assert!(
        rendered.contains("[providers.claude_cli]"),
        "a whole-file rewrite dropped the provider the user configured:\n{rendered}"
    );
    assert!(
        !rendered.contains("[providers.xiaomi]"),
        "a whole-file rewrite added a provider the user never configured:\n{rendered}"
    );
}
