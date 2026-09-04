//! Regression tests for entrypoint brain seeding (#1382).
//!
//! Bug: brain templates only reached disk when the onboarding wizard
//! completed. Daemon-only, docker, channel-first, `init`, and aborted
//! wizard installs ran with an empty brain forever — users saw a
//! personality-less agent. Fix: `ensure_brain_seeded()` at every CLI
//! entrypoint, reusing the never-overwrite `seed_brain_templates`.
//!
//! These tests drive the REAL `ensure_brain_seeded()` (resolution included)
//! through the profile-home override into a throwaway profile, so they
//! never touch the developer's actual `~/.opencrabs/`.

const ALL_NINE: [&str; 9] = [
    "SOUL.md",
    "USER.md",
    "AGENTS.md",
    "TOOLS.md",
    "MEMORY.md",
    "CODE.md",
    "SECURITY.md",
    "BOOT.md",
    "HEARTBEAT.md",
];

fn throwaway_home(tag: &str) -> std::path::PathBuf {
    let profile = format!("brain-seed-test-{tag}");
    let home = crate::config::profile::home_for_profile(Some(&profile));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    home
}

#[test]
fn empty_home_gets_full_brain_on_first_open() {
    let home = throwaway_home("full");
    crate::config::profile::with_profile_home(Some("brain-seed-test-full"), || {
        crate::config::profile::ensure_brain_seeded();
    });
    for f in ALL_NINE {
        assert!(home.join(f).exists(), "first open must seed {f}");
    }
    // Belief base rides along (#881) — without it the Orient gate is inert.
    assert!(
        home.join("safety").join("brain_verify.toml").exists(),
        "first open must seed safety/brain_verify.toml"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn reseed_never_overwrites_user_content() {
    let home = throwaway_home("keep");
    crate::config::profile::with_profile_home(Some("brain-seed-test-keep"), || {
        crate::config::profile::ensure_brain_seeded();
    });
    let soul = home.join("SOUL.md");
    std::fs::write(&soul, "MY HAND-EDITED SOUL — do not clobber").unwrap();
    crate::config::profile::with_profile_home(Some("brain-seed-test-keep"), || {
        crate::config::profile::ensure_brain_seeded(); // second boot
    });
    assert_eq!(
        std::fs::read_to_string(&soul).unwrap(),
        "MY HAND-EDITED SOUL — do not clobber",
        "re-seeding must never overwrite user content"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn partial_brain_completed_without_touching_custom_soul() {
    // The exact user report shape: the wizard's BrainSetup wrote an
    // AI-personalized SOUL.md, then onboarding aborted before finalize —
    // so ONLY SOUL.md exists. Entrypoint seeding must complete the rest
    // of the brain while leaving the personality file untouched.
    let home = throwaway_home("partial");
    std::fs::write(home.join("SOUL.md"), "AI-GENERATED PERSONALITY").unwrap();
    crate::config::profile::with_profile_home(Some("brain-seed-test-partial"), || {
        crate::config::profile::ensure_brain_seeded();
    });
    assert_eq!(
        std::fs::read_to_string(home.join("SOUL.md")).unwrap(),
        "AI-GENERATED PERSONALITY",
        "AI-generated personality must survive entrypoint seeding"
    );
    for f in ALL_NINE {
        assert!(
            home.join(f).exists(),
            "partial brain must be completed: {f}"
        );
    }
    let _ = std::fs::remove_dir_all(&home);
}
