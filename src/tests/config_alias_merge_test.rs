//! Both spellings of one section parse as one (#1116).
//!
//! `Config` reaches the A2A settings through `#[serde(alias = "gateway")]`.
//! serde reads an alias as another spelling of the same field, not a second
//! field, so a file carrying both `[gateway]` and `[a2a]` failed with
//! `duplicate field` reported against line 1. The load path treated that as a
//! syntax error and fell back to the last-known-good snapshot, so the instance
//! ran on a stale copy for days and every later edit appeared to do nothing.

use crate::config::alias_merge::fold_legacy_sections;

fn doc(s: &str) -> toml::Value {
    toml::from_str(s).expect("valid TOML")
}

#[test]
fn both_sections_fold_into_one() {
    // The reported file shape.
    let mut d = doc("[gateway]\nenabled = true\n\n[a2a]\nport = 9000\n");
    let folded = fold_legacy_sections(&mut d);

    assert_eq!(folded, vec!["gateway"]);
    let t = d.as_table().unwrap();
    assert!(!t.contains_key("gateway"), "legacy key is consumed");
    let a2a = t.get("a2a").unwrap().as_table().unwrap();
    assert_eq!(a2a.get("enabled").unwrap().as_bool(), Some(true));
    assert_eq!(a2a.get("port").unwrap().as_integer(), Some(9000));
}

#[test]
fn the_canonical_value_wins_a_per_key_conflict() {
    // A value under the current name is the more deliberate of the two.
    let mut d = doc("[gateway]\nport = 1111\n\n[a2a]\nport = 2222\n");
    fold_legacy_sections(&mut d);

    let a2a = d
        .as_table()
        .unwrap()
        .get("a2a")
        .unwrap()
        .as_table()
        .unwrap();
    assert_eq!(a2a.get("port").unwrap().as_integer(), Some(2222));
}

#[test]
fn the_legacy_spelling_alone_is_renamed() {
    // What most existing configs look like, including the shipped example.
    let mut d = doc("[gateway]\nenabled = true\n");
    let folded = fold_legacy_sections(&mut d);

    assert_eq!(folded, vec!["gateway"]);
    let t = d.as_table().unwrap();
    assert!(!t.contains_key("gateway"));
    assert_eq!(
        t.get("a2a")
            .unwrap()
            .as_table()
            .unwrap()
            .get("enabled")
            .unwrap()
            .as_bool(),
        Some(true)
    );
}

#[test]
fn the_canonical_spelling_alone_is_untouched() {
    let mut d = doc("[a2a]\nenabled = true\n");
    let folded = fold_legacy_sections(&mut d);

    assert!(folded.is_empty(), "nothing to fold, so nothing reported");
    assert!(d.as_table().unwrap().contains_key("a2a"));
}

#[test]
fn a_file_with_neither_is_left_alone() {
    let mut d = doc("[providers]\nx = 1\n");
    assert!(fold_legacy_sections(&mut d).is_empty());
    assert!(d.as_table().unwrap().contains_key("providers"));
}

#[test]
fn nested_tables_merge_rather_than_replace() {
    let mut d = doc(
        "[gateway.auth]\ntoken = \"legacy\"\nscheme = \"bearer\"\n\n\
         [a2a.auth]\ntoken = \"current\"\n",
    );
    fold_legacy_sections(&mut d);

    let auth = d.as_table().unwrap()["a2a"].as_table().unwrap()["auth"]
        .as_table()
        .unwrap();
    assert_eq!(auth.get("token").unwrap().as_str(), Some("current"));
    assert_eq!(
        auth.get("scheme").unwrap().as_str(),
        Some("bearer"),
        "a key only the legacy section had must survive the merge"
    );
}

#[test]
fn the_whole_config_deserializes_with_both_sections_present() {
    // End to end: this is the case that previously failed with
    // `duplicate field a2a` and stranded the instance on a stale snapshot.
    let mut d = doc("[gateway]\nenabled = true\n\n[a2a]\nport = 9000\n");
    fold_legacy_sections(&mut d);
    let cfg: Result<crate::config::Config, _> = d.try_into();
    assert!(
        cfg.is_ok(),
        "both sections must parse as one: {:?}",
        cfg.err()
    );
}

// ── On-disk migration (#1116) ────────────────────────────────────────
//
// The fold above keeps both spellings loading, but leaves the file as
// written, so an old config keeps the legacy name forever. Migration
// renames it once so the two names actually converge.

use crate::config::alias_merge::migrate_file;

fn write_tmp(name: &str, body: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "opencrabs-alias-migrate-{}-{}.toml",
        name,
        std::process::id()
    ));
    std::fs::write(&p, body).expect("write temp config");
    p
}

#[test]
fn the_legacy_section_is_renamed_on_disk() {
    let p = write_tmp("rename", "[gateway]\nenabled = true\nport = 18790\n");

    let renamed = migrate_file(&p).expect("migrate");

    assert_eq!(renamed, vec!["gateway"]);
    let after = std::fs::read_to_string(&p).unwrap();
    assert!(after.contains("[a2a]"), "renamed to the current name");
    assert!(!after.contains("[gateway]"), "legacy name is gone");
    assert!(
        after.contains("port = 18790"),
        "settings survive the rename"
    );
    let _ = std::fs::remove_file(&p);
}

#[test]
fn comments_survive_the_migration() {
    // These files are mostly comments. Losing them would cost the user every
    // note they had written, which is worse than the problem being fixed.
    let p = write_tmp(
        "comments",
        "# top of file\n\n# what this section does\n[gateway]\nenabled = true  # inline note\n",
    );

    migrate_file(&p).expect("migrate");

    let after = std::fs::read_to_string(&p).unwrap();
    assert!(after.contains("# top of file"));
    assert!(after.contains("# what this section does"));
    assert!(after.contains("# inline note"));
    let _ = std::fs::remove_file(&p);
}

#[test]
fn a_file_with_both_sections_is_merged_into_one_on_disk() {
    let p = write_tmp(
        "both",
        "[gateway]\nenabled = true\nport = 1111\n\n[a2a]\nport = 2222\n",
    );

    let renamed = migrate_file(&p).expect("migrate");

    assert_eq!(renamed, vec!["gateway"]);
    let after = std::fs::read_to_string(&p).unwrap();
    assert!(!after.contains("[gateway]"));
    assert!(after.contains("enabled = true"), "legacy-only key is kept");
    assert!(after.contains("2222"), "canonical value wins the conflict");
    assert!(!after.contains("1111"), "legacy value loses the conflict");
    let _ = std::fs::remove_file(&p);
}

#[test]
fn a_file_already_using_the_current_name_is_left_alone() {
    let body = "[a2a]\nenabled = true\n";
    let p = write_tmp("noop", body);

    let renamed = migrate_file(&p).expect("migrate");

    assert!(renamed.is_empty(), "nothing renamed, so nothing reported");
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        body,
        "an untouched file must be byte-identical, not reformatted"
    );
    let _ = std::fs::remove_file(&p);
}

#[test]
fn an_unparseable_file_is_not_rewritten() {
    // Not ours to mangle: the loader reports the parse error instead.
    let body = "[gateway\nenabled = true\n";
    let p = write_tmp("broken", body);

    let renamed = migrate_file(&p).expect("migrate must not error");

    assert!(renamed.is_empty());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), body);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn migrating_twice_is_a_no_op_the_second_time() {
    let p = write_tmp("idempotent", "[gateway]\nenabled = true\n");

    assert_eq!(migrate_file(&p).expect("first"), vec!["gateway"]);
    let after_first = std::fs::read_to_string(&p).unwrap();
    assert!(migrate_file(&p).expect("second").is_empty());
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        after_first,
        "a second run must not touch the file again"
    );
    let _ = std::fs::remove_file(&p);
}

#[test]
fn a_real_world_config_migrates_without_losing_anything() {
    // Guards the shape that actually ships: many sections, many comments,
    // one legacy section among them. A rename that quietly dropped unrelated
    // content would be far worse than the naming problem it fixes.
    let body = "\
# OpenCrabs config\n\
\n\
[agent]\n\
max_tokens = 65536  # keep\n\
\n\
# ========================================\n\
# Agent-to-Agent (A2A) Protocol\n\
# ========================================\n\
[gateway]\n\
enabled = false\n\
port = 18790\n\
\n\
[channels.telegram]\n\
enabled = true\n";
    let p = write_tmp("realworld", body);

    let renamed = migrate_file(&p).expect("migrate");
    assert_eq!(renamed, vec!["gateway"]);

    let after = std::fs::read_to_string(&p).unwrap();
    // The rename happened.
    assert!(after.contains("[a2a]") && !after.contains("[gateway]"));
    // Every other section survived, in place.
    assert!(after.contains("[agent]"));
    assert!(after.contains("max_tokens = 65536"));
    assert!(after.contains("[channels.telegram]"));
    // And so did the banner comments around the renamed section.
    assert!(after.contains("# Agent-to-Agent (A2A) Protocol"));
    assert!(after.contains("# keep"));
    let _ = std::fs::remove_file(&p);
}
