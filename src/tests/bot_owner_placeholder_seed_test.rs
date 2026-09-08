//! A documentation placeholder must never become a bot owner.
//!
//! `config.toml.example` ships an example id in each channel's allow-list to
//! show the field's shape, and that file is seeded verbatim into a real
//! `config.toml`. The `bot_owner` seed migration then promoted the first
//! allow-list entry to an explicit owner. Because an explicit `bot_owner` IS
//! the owner set, the positional fallback that makes the first real user the
//! owner could never fire: the operator was silently not the owner of their
//! own bot, and whoever held the account matching the placeholder was (#1440).
//!
//! Three guards, matching the three ways this went wrong: the template ships
//! no armed allow-list, the migration refuses a placeholder even if one
//! reaches it, and it does not pin an owner on a channel nobody enabled.

use crate::config::Config;

/// The template as it ships, which is what onboarding writes to disk.
const TEMPLATE: &str = include_str!("../../config.toml.example");

/// An allow-list line that is live rather than commented out.
fn armed_allow_list(line: &str) -> bool {
    let t = line.trim_start();
    (t.starts_with("allowed_users") || t.starts_with("allowed_phones")) && !t.contains("= []")
}

#[test]
fn the_template_ships_no_armed_allow_list() {
    let armed: Vec<(usize, &str)> = TEMPLATE
        .lines()
        .enumerate()
        .filter(|(_, l)| armed_allow_list(l))
        .map(|(i, l)| (i + 1, l.trim()))
        .collect();

    assert!(
        armed.is_empty(),
        "config.toml.example is copied verbatim into a real config, so an \
         uncommented allow-list ships a stranger's id as an ACL entry:\n{armed:#?}"
    );
}

#[test]
fn template_placeholders_are_all_listed() {
    // Every example id the template shows must be one the migration refuses.
    // Pinned against the template itself so an id added there cannot drift
    // past the refusal list unnoticed.
    for line in TEMPLATE.lines() {
        let t = line.trim_start().trim_start_matches("# ");
        if !t.starts_with("allowed_users") && !t.starts_with("allowed_phones") {
            continue;
        }
        let Some(list) = t.split_once('=').map(|(_, r)| r) else {
            continue;
        };
        let Some(inner) = list.split('[').nth(1).and_then(|r| r.split(']').next()) else {
            continue;
        };
        for raw in inner.split(',') {
            let value = raw.trim().trim_matches('"').trim();
            if value.is_empty() {
                continue;
            }
            assert!(
                Config::OWNER_SEED_PLACEHOLDERS.contains(&value),
                "the template shows {value:?} as an example id, but the migration \
                 does not treat it as a placeholder, so seeding it would make it a \
                 real owner. Add it to OWNER_SEED_PLACEHOLDERS."
            );
        }
    }
}

#[test]
fn placeholder_values_cover_every_shape_the_template_uses() {
    // A string phone, a long numeric snowflake, a short numeric id, a string
    // handle. Losing any shape silently narrows the refusal.
    assert!(Config::OWNER_SEED_PLACEHOLDERS.contains(&"+15551234567"));
    assert!(Config::OWNER_SEED_PLACEHOLDERS.contains(&"123456789012345"));
    assert!(Config::OWNER_SEED_PLACEHOLDERS.contains(&"123456789"));
    assert!(Config::OWNER_SEED_PLACEHOLDERS.contains(&"U12345678"));
}
