//! Sentiment classification and synthetic-prompt framing for inbound Telegram
//! reactions (#302 Stage 1).
//!
//! When a user reacts to one of the bot's messages, `handle_reaction` turns the
//! emoji into an agent turn. These helpers give that turn meaning: a positive
//! reaction reads as approval / "on the right path, keep going", a negative one
//! as "pause and ask what to change", and every prompt addresses the person by
//! their first name so the acknowledgement lands naturally.
//!
//! Pure and channel-agnostic so it stays unit-testable without a live bot.

/// How an inbound reaction reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReactionSentiment {
    /// Encouragement / approval — keep going or proceed with what was proposed.
    Positive,
    /// A stop signal — pause and ask for feedback.
    Negative,
    /// Anything else — acknowledge naturally, no strong steer.
    Neutral,
}

/// Emojis that read as encouragement or approval.
const POSITIVE: &[&str] = &["👍", "👌", "💪", "🫡", "🆗", "🔥", "💯"];

/// Emojis that read as "stop / something's wrong". `⛔` is listed both with and
/// without the U+FE0F variation selector, since Telegram may deliver either.
const NEGATIVE: &[&str] = &["⛔️", "⛔", "🚫", "🛑", "👎"];

/// Classify a reaction emoji into a [`ReactionSentiment`].
pub(crate) fn classify_reaction(emoji: &str) -> ReactionSentiment {
    let trimmed = emoji.trim();
    if POSITIVE.contains(&trimmed) {
        ReactionSentiment::Positive
    } else if NEGATIVE.contains(&trimmed) {
        ReactionSentiment::Negative
    } else {
        ReactionSentiment::Neutral
    }
}

/// Build the synthetic prompt handed to the agent when `first_name` reacts with
/// `emoji` to the bot message previewed by `preview`. Frames the sentiment and
/// tells the agent to address the person by first name.
pub(crate) fn build_reaction_prompt(first_name: &str, emoji: &str, preview: &str) -> String {
    let steer = match classify_reaction(emoji) {
        ReactionSentiment::Positive => format!(
            "This reads as approval from {first_name}: you're on the right path. Acknowledge \
             {first_name} by first name and keep the momentum. If you had proposed a next \
             step or a turn is in progress, treat it as a green light to proceed."
        ),
        ReactionSentiment::Negative => format!(
            "This reads as {first_name} flagging that something is off. Address {first_name} \
             by first name, pause rather than press on, and ask what they'd like changed."
        ),
        ReactionSentiment::Neutral => {
            format!("Acknowledge {first_name} by first name and respond naturally.")
        }
    };
    format!(
        "[Reaction notification] {first_name} reacted with {emoji} to your message:\n\
         \"{preview}\"\n\n\
         {steer}\n\n\
         You may react back (use <<react:EMOJI>>), reply with text, or do both. If the \
         reaction doesn't warrant a text response, reply with <<react:{emoji}>> to silently \
         acknowledge."
    )
}
