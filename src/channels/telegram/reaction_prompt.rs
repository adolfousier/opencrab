//! Sentiment classification and synthetic-prompt framing for inbound Telegram
//! reactions (#302 Stage 1).
//!
//! When a user reacts to one of the bot's messages, `handle_reaction` turns the
//! emoji into an agent turn. These helpers give that turn meaning: a positive
//! reaction reads as approval / "on the right path, keep going", a negative one
//! as "pause and ask what to change".
//!
//! A reaction is a lightweight acknowledgement, not a request, so the default
//! response is to react back with a single emoji and NO text. Answering every
//! reaction with prose is noise, and in a group where several people react it
//! multiplies into spam. Only a negative reaction (someone flagging a problem)
//! genuinely warrants a text reply.
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
/// `emoji` to the bot message previewed by `preview`. React-only is the default;
/// only a negative reaction warrants text. `is_group` hardens the react-only
/// steer, since in a group a text reply per reaction is noise for everyone else.
pub(crate) fn build_reaction_prompt(
    first_name: &str,
    emoji: &str,
    preview: &str,
    is_group: bool,
) -> String {
    let steer = match classify_reaction(emoji) {
        ReactionSentiment::Negative => format!(
            "This reads as {first_name} flagging that something is off. This is the one case \
             that warrants a short reply: address {first_name} by first name, pause rather than \
             press on, and ask what they'd like changed."
        ),
        ReactionSentiment::Positive | ReactionSentiment::Neutral => {
            let group_note = if is_group {
                " This is a group, so a text reply is noise for everyone else: react-only unless \
                 a text response is genuinely necessary."
            } else {
                ""
            };
            format!(
                "This is a lightweight acknowledgement from {first_name}, not a request. Default \
                 to reacting back with a single emoji and NO text.{group_note} Only write text if \
                 you have something genuinely new and useful to add, which a reaction almost never \
                 calls for."
            )
        }
    };
    format!(
        "[Reaction notification] {first_name} reacted with {emoji} to your message:\n\
         \"{preview}\"\n\n\
         {steer}\n\n\
         To acknowledge silently, reply with ONLY <<react:EMOJI>> (e.g. <<react:🙏>> or \
         <<react:{emoji}>>) and no other text. That is the expected response for a routine \
         reaction."
    )
}

/// Framing for a reaction that lands while a turn is already running, injected
/// into the loop between tool rounds as live steering (#302 Stage 2). Unlike
/// [`build_reaction_prompt`] this is not a fresh turn — it is an out-of-band
/// signal telling the in-flight agent how the person feels about the work so
/// far, so the framing makes clear it arrived mid-work.
pub(crate) fn build_midturn_reaction_message(first_name: &str, emoji: &str) -> String {
    match classify_reaction(emoji) {
        ReactionSentiment::Positive => format!(
            "[Live feedback from {first_name}: reacted {emoji}] They approve of the current \
             direction, you're on the right path. Keep going with the current plan, no need to \
             stop or re-confirm. Acknowledge {first_name} by first name when you next reply."
        ),
        ReactionSentiment::Negative => format!(
            "[Live feedback from {first_name}: reacted {emoji}] They want you to PAUSE. Stop at \
             the next safe point, summarize what you've done so far, and ask {first_name} (by \
             first name) what they'd like changed before continuing."
        ),
        ReactionSentiment::Neutral => format!(
            "[Live feedback from {first_name}: reacted {emoji}] Take it into account and \
             acknowledge {first_name} by first name when you next reply."
        ),
    }
}
