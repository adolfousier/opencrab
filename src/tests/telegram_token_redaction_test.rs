//! The Telegram bot token must never reach a log.
//!
//! The Bot API carries the token in the URL path and `reqwest`'s error
//! `Display` includes the failing URL, so `tracing::warn!("...: {e}")` on any
//! Telegram call used to write the token to disk in plaintext (#1322). Anyone
//! with the log directory, or handed a log excerpt in a bug report, had the
//! bot.

use crate::logging::redact::{REDACTED, contains_token, scrub};

/// A synthetic token in the Bot API's shape: numeric id, colon, then the
/// secret. Not a real credential.
const FAKE_TOKEN: &str = "bot1234567890:AAFakeTokenValueForTestingOnly12345";

/// The exact line shape that was leaking, from the measured `flow.rs` site.
fn leaking_log_line() -> String {
    format!(
        "Telegram: rich details edit failed for mid=MessageId(16685): error sending \
         request for url (https://api.telegram.org/{FAKE_TOKEN}/editMessageText) \
         — falling back to HTML"
    )
}

#[test]
fn a_formatted_transport_error_carries_no_token() {
    let line = leaking_log_line();
    let scrubbed = scrub(&line);
    assert!(
        !contains_token(&scrubbed),
        "token survived redaction: {scrubbed}"
    );
    assert!(
        !scrubbed.contains("AAFakeTokenValueForTestingOnly12345"),
        "the secret itself must be gone: {scrubbed}"
    );
}

/// The numeric id is the bot's public identity, not the secret. Keeping it
/// means a redacted line still says which bot failed.
#[test]
fn the_bot_id_survives_so_the_line_stays_diagnosable() {
    let line = leaking_log_line();
    let scrubbed = scrub(&line);
    assert!(scrubbed.contains("bot1234567890"), "{scrubbed}");
    assert!(scrubbed.contains(REDACTED), "{scrubbed}");
    // Everything around the token is untouched.
    assert!(scrubbed.contains("MessageId(16685)"));
    assert!(scrubbed.contains("api.telegram.org"));
    assert!(scrubbed.contains("editMessageText"));
}

#[test]
fn every_token_in_a_line_is_redacted() {
    let two = format!("first {FAKE_TOKEN}/sendMessage then {FAKE_TOKEN}/editMessageText");
    let scrubbed = scrub(&two);
    assert!(!contains_token(&scrubbed), "{scrubbed}");
    assert_eq!(scrubbed.matches(REDACTED).count(), 2, "{scrubbed}");
}

/// The common case is a line with no token at all, which must not allocate.
#[test]
fn an_ordinary_line_is_borrowed_untouched() {
    let line = "Telegram send ok: kind=rich_edit chat=-100123 thread=None msg=42";
    assert!(matches!(scrub(line), std::borrow::Cow::Borrowed(_)));
    assert_eq!(scrub(line), line);
}

/// Prose that merely contains "bot" is not a token and must survive intact:
/// the secret half is what identifies one, and it is long.
#[test]
fn prose_mentioning_a_bot_is_not_mangled() {
    for line in [
        "the bot is typing",
        "bot1:ok",
        "robot:starting",
        "bot 1234567890: something happened",
    ] {
        assert_eq!(scrub(line), line, "should be untouched: {line}");
    }
}

/// The scrubbing writer reports the caller's byte count, not the shorter
/// redacted one. Returning the short count would make `write_all` re-send the
/// tail and duplicate part of the line.
#[test]
fn the_writer_reports_the_input_length_after_shrinking_a_line() {
    use std::io::Write;
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Captured;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    let sink = Captured::default();
    let made = crate::logging::logger::ScrubbingWriter(sink.clone());
    let line = leaking_log_line();

    let written = made.make_writer().write(line.as_bytes()).unwrap();
    assert_eq!(
        written,
        line.len(),
        "must report the caller's length so write_all does not resend the tail"
    );

    let out = sink.0.lock().unwrap().clone();
    let out = String::from_utf8(out).unwrap();
    assert!(!contains_token(&out), "token reached the sink: {out}");
    assert!(out.len() < line.len(), "redacted output should be shorter");
}
