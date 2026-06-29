use crate::usage::data::*;

#[test]
fn test_period_cycle() {
    assert_eq!(Period::Today.next(), Period::Week);
    assert_eq!(Period::Week.next(), Period::Month);
    assert_eq!(Period::Month.next(), Period::AllTime);
    assert_eq!(Period::AllTime.next(), Period::Today);
}

#[test]
fn test_period_since_epoch() {
    assert!(Period::Today.since_epoch().is_some());
    assert!(Period::Week.since_epoch().is_some());
    assert!(Period::Month.since_epoch().is_some());
    assert!(Period::AllTime.since_epoch().is_none());
}

#[test]
fn test_period_labels() {
    assert_eq!(Period::Today.label(), "Today");
    assert_eq!(Period::Week.label(), "Week");
    assert_eq!(Period::Month.label(), "Month");
    assert_eq!(Period::AllTime.label(), "All Time");
}

#[test]
fn test_classify_activity() {
    assert_eq!(classify_activity("fix login bug"), "Bug Fixes");
    assert_eq!(classify_activity("Fix crash on startup"), "Bug Fixes");
    assert_eq!(
        classify_activity("error handling improvements"),
        "Bug Fixes"
    );
    assert_eq!(classify_activity("refactor auth module"), "Refactoring");
    assert_eq!(classify_activity("cleanup old code"), "Refactoring");
    assert_eq!(classify_activity("add unit tests"), "Testing");
    assert_eq!(classify_activity("test coverage for parser"), "Testing");
    assert_eq!(classify_activity("update README"), "Documentation");
    assert_eq!(classify_activity("changelog updates"), "Documentation");
    assert_eq!(classify_activity("ci pipeline fix"), "CI/Deploy");
    assert_eq!(classify_activity("release v1.0"), "CI/Deploy");
    assert_eq!(classify_activity("deploy to prod"), "CI/Deploy");
    assert_eq!(classify_activity("add new feature"), "Features");
    assert_eq!(classify_activity("implement search"), "Features");
    assert_eq!(classify_activity("config file parsing"), "Config");
    assert_eq!(classify_activity("setup dev environment"), "Config");
    assert_eq!(classify_activity("random chat session"), "Development");
    assert_eq!(classify_activity(""), "Development");
}

#[test]
fn test_fmt_tokens() {
    assert_eq!(fmt_tokens(0), "0");
    assert_eq!(fmt_tokens(500), "500");
    assert_eq!(fmt_tokens(1_500), "2K");
    assert_eq!(fmt_tokens(1_500_000), "1.5M");
    assert_eq!(fmt_tokens(1_292_500_000), "1292.5M");
}

#[test]
fn test_fmt_cost() {
    assert_eq!(fmt_cost(0.0), "$0.0000");
    assert_eq!(fmt_cost(0.005), "$0.0050");
    assert_eq!(fmt_cost(0.05), "$0.050");
    assert_eq!(fmt_cost(1.50), "$1.50");
    assert_eq!(fmt_cost(507.20), "$507.20");
}

#[test]
fn test_dashboard_data_default() {
    let d = DashboardData::default();
    assert_eq!(d.summary.total_tokens, 0);
    assert_eq!(d.summary.total_cost, 0.0);
    assert!(d.daily.is_empty());
    assert!(d.projects.is_empty());
    assert!(d.models.is_empty());
    assert!(d.tools.is_empty());
    assert!(d.activities.is_empty());
    assert!(d.cache.is_none());
}
