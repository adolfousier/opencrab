//! A quoted secret must redact its contents (#879).
//!
//! The prefix matcher treated a quote as the END of the secret, so for
//! `TOKEN="value"` the delimiter search returned position 0, the
//! `secret_end > after` guard failed, and NOTHING was redacted. The unquoted
//! form worked, so the failing case was the more common shell spelling.
//!
//! Two structural gaps let it through unnoticed: no pattern matched the
//! `<digits>:<opaque>` provider-token shape, and the env-assignment rule
//! required a suffixed name so bare `TOKEN=` never fired.
//!
//! Fixtures are synthetic. The token-shaped strings below are invented and
//! carry no real credential.

use crate::utils::sanitize::redact_secrets;

/// Shaped like a Telegram bot token; not a real one.
const SHAPED: &str = "8478243969:AAEbZSu8-VGp8opQIsxxFAKEfakeFAKE0000000";

#[test]
fn a_double_quoted_secret_is_redacted() {
    // The exact failing form.
    let out = redact_secrets(&format!("TOKEN=\"{SHAPED}\""));
    assert!(!out.contains(SHAPED), "quoted secret survived: {out}");
}

#[test]
fn a_single_quoted_secret_is_redacted() {
    let out = redact_secrets(&format!("TOKEN='{SHAPED}'"));
    assert!(!out.contains(SHAPED), "quoted secret survived: {out}");
}

#[test]
fn the_unquoted_form_still_works() {
    // Guard against fixing one spelling by breaking the other.
    let out = redact_secrets(&format!("TOKEN={SHAPED} rest"));
    assert!(!out.contains(SHAPED), "unquoted secret survived: {out}");
}

#[test]
fn a_space_inside_quotes_does_not_cut_the_redaction_short() {
    // Quoted values may contain spaces; terminating on the first space would
    // leave the tail exposed.
    let out = redact_secrets("PASSWORD=\"two words here\"");
    assert!(!out.contains("two words here"), "tail leaked: {out}");
}

#[test]
fn the_token_shape_is_caught_without_any_prefix() {
    // The structural net: no assignment, no prefix, just the value in prose.
    // MIXED_ALNUM cannot see this shape because the colon and hyphens break
    // the alphanumeric run.
    let out = redact_secrets(&format!("the value is {SHAPED} apparently"));
    assert!(!out.contains(SHAPED), "bare token shape survived: {out}");
}

#[test]
fn a_bare_sensitive_name_is_covered() {
    // ENV_ASSIGN previously required a suffix, so BOT_TOKEN matched and plain
    // TOKEN did not.
    for name in ["TOKEN", "SECRET", "PASSWORD", "APIKEY"] {
        let out = redact_secrets(&format!("{name}=\"{SHAPED}\""));
        assert!(
            !out.contains(SHAPED),
            "{name} left the value exposed: {out}"
        );
    }
}

#[test]
fn a_suffixed_name_still_works() {
    let out = redact_secrets(&format!("BOT_TOKEN=\"{SHAPED}\""));
    assert!(!out.contains(SHAPED), "suffixed name regressed: {out}");
}

#[test]
fn ordinary_text_is_not_redacted() {
    // The widened rules must not start eating prose or normal commands.
    let text = "ratio=3:4 and time=12:30 with key=value in a sentence";
    let out = redact_secrets(text);
    assert!(
        out.contains("3:4"),
        "harmless colon pair was redacted: {out}"
    );
    assert!(out.contains("12:30"), "a timestamp was redacted: {out}");
}

#[test]
fn a_url_path_segment_is_left_alone() {
    // Long path segments are not secrets; the existing guard must survive.
    let text = "https://example.com/v1/8478243969/status";
    assert!(redact_secrets(text).contains("8478243969"));
}
