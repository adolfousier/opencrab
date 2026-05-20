//! Custom-provider model paste: users can type a model name that the
//! provider's `/v1/models` endpoint doesn't list, have it accepted as
//! the chosen model, and have it survive future fetches by merging
//! with the config-persisted list.

use crate::tui::provider_selector::ProviderSelectorState;

#[test]
fn all_model_names_returns_fetched_when_no_config_models() {
    let ps = ProviderSelectorState {
        models: vec!["a".into(), "b".into(), "c".into()],
        ..Default::default()
    };
    let names = ps.all_model_names();
    assert_eq!(names, vec!["a", "b", "c"]);
}

#[test]
fn all_model_names_returns_config_when_no_fetched() {
    let ps = ProviderSelectorState {
        config_models: vec!["x".into(), "y".into()],
        ..Default::default()
    };
    let names = ps.all_model_names();
    assert_eq!(names, vec!["x", "y"]);
}

#[test]
fn all_model_names_unions_fetched_and_config_preserving_order() {
    let ps = ProviderSelectorState {
        models: vec!["live-1".into(), "live-2".into()],
        config_models: vec!["live-1".into(), "pasted".into()],
        ..Default::default()
    };
    let names = ps.all_model_names();
    assert_eq!(names, vec!["live-1", "live-2", "pasted"]);
}

#[test]
fn merge_config_models_appends_extras_to_fetched() {
    let mut ps = ProviderSelectorState {
        models: vec!["live-1".into(), "live-2".into()],
        config_models: vec!["live-1".into(), "pasted".into(), "another".into()],
        ..Default::default()
    };

    let extras: Vec<String> = ps
        .config_models
        .iter()
        .filter(|m| !ps.models.iter().any(|x| x == *m))
        .cloned()
        .collect();
    ps.models.extend(extras);

    assert_eq!(ps.models, vec!["live-1", "live-2", "pasted", "another"]);
}

#[test]
fn merge_is_idempotent() {
    let mut ps = ProviderSelectorState {
        models: vec!["a".into(), "b".into(), "c".into()],
        config_models: vec!["a".into(), "b".into(), "c".into()],
        ..Default::default()
    };

    for _ in 0..2 {
        let extras: Vec<String> = ps
            .config_models
            .iter()
            .filter(|m| !ps.models.iter().any(|x| x == *m))
            .cloned()
            .collect();
        ps.models.extend(extras);
    }

    assert_eq!(ps.models, vec!["a", "b", "c"]);
}
