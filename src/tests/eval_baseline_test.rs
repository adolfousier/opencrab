//! Tests for the regression baseline (#624).

use std::collections::BTreeMap;

use crate::eval::baseline::{Baseline, OVERALL_KEY};
use crate::eval::scorer::{BinaryQuestion, BinaryVerdict, Scorecard};

fn baseline(label: &str, overall: f64, dims: &[(&str, f64)]) -> Baseline {
    Baseline {
        label: label.to_string(),
        overall,
        dimensions: dims.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
    }
}

#[test]
fn from_scorecard_captures_overall_and_dimensions() {
    // prefs: 1/2, tasks: 1/1 -> overall 2/3.
    let verdict = |yes| BinaryVerdict {
        yes,
        explanation: None,
    };
    let results = vec![
        (BinaryQuestion::new("prefs", "q1"), verdict(true)),
        (BinaryQuestion::new("prefs", "q2"), verdict(false)),
        (BinaryQuestion::new("tasks", "q3"), verdict(true)),
    ];
    let card = Scorecard::from_verdicts(results);
    let base = Baseline::from_scorecard("run-1", &card);
    assert_eq!(base.dimensions["prefs"], 0.5);
    assert_eq!(base.dimensions["tasks"], 1.0);
    assert!((base.overall - 2.0 / 3.0).abs() < 1e-9);
}

#[test]
fn no_change_holds() {
    let base = baseline("b", 0.8, &[("prefs", 0.9), ("tasks", 0.7)]);
    let current = base.clone();
    assert!(base.holds(&current, 0.02));
    assert!(base.regressions(&current, 0.02).is_empty());
}

#[test]
fn drop_beyond_tolerance_flags_the_dimension() {
    let base = baseline("b", 0.80, &[("prefs", 0.90), ("tasks", 0.70)]);
    // tasks fell 0.70 -> 0.50 (0.20 drop); prefs unchanged; overall 0.80 -> 0.70.
    let current = baseline("c", 0.70, &[("prefs", 0.90), ("tasks", 0.50)]);
    let regs = base.regressions(&current, 0.05);
    let dims: Vec<&str> = regs.iter().map(|r| r.dimension.as_str()).collect();
    assert!(dims.contains(&"tasks"));
    assert!(dims.contains(&OVERALL_KEY));
    assert!(!dims.contains(&"prefs"));
    let tasks = regs.iter().find(|r| r.dimension == "tasks").unwrap();
    assert!((tasks.delta - (-0.20)).abs() < 1e-9);
    assert!(!base.holds(&current, 0.05));
}

#[test]
fn improvement_never_flags() {
    let base = baseline("b", 0.60, &[("prefs", 0.60)]);
    let current = baseline("c", 0.95, &[("prefs", 1.0)]);
    assert!(base.holds(&current, 0.0));
}

#[test]
fn missing_dimension_counts_as_drop_to_zero() {
    let base = baseline("b", 0.80, &[("prefs", 0.9), ("tasks", 0.8)]);
    // tasks no longer measured.
    let current = baseline("c", 0.80, &[("prefs", 0.9)]);
    let regs = base.regressions(&current, 0.05);
    assert!(
        regs.iter()
            .any(|r| r.dimension == "tasks" && r.current == 0.0)
    );
}

#[test]
fn json_round_trip_is_stable() {
    let base = baseline("run", 0.75, &[("a", 0.5), ("b", 1.0)]);
    let json = base.to_json().unwrap();
    let back = Baseline::from_json(&json).unwrap();
    assert_eq!(back.overall, 0.75);
    assert_eq!(back.dimensions, {
        let mut m = BTreeMap::new();
        m.insert("a".to_string(), 0.5);
        m.insert("b".to_string(), 1.0);
        m
    });
}
