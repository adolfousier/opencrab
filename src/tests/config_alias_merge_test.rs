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
