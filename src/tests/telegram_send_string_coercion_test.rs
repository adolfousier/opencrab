//! #646: schema says "integer" but models often quote the value. Every
//! numeric param in telegram_send now coerces numeric strings instead of
//! silently dropping them (required-param error or session-fallback misroute).

use crate::brain::tools::telegram_send::{value_as_f64, value_as_i64};
use serde_json::{Value, json};

#[test]
fn coerces_quoted_integers() {
    assert_eq!(value_as_i64(&json!("123456")), Some(123456));
    assert_eq!(value_as_i64(&json!("-7")), Some(-7));
    // Native integers pass through untouched.
    assert_eq!(value_as_i64(&json!(42)), Some(42));
}

#[test]
fn rejects_non_numeric_garbage() {
    assert_eq!(value_as_i64(&json!("not-a-number")), None);
    assert_eq!(value_as_i64(&Value::Null), None);
    assert_eq!(value_as_i64(&json!({})), None);
}

#[test]
fn coerces_quoted_floats() {
    assert_eq!(value_as_f64(&json!("40.6333")), Some(40.6333));
    assert_eq!(value_as_f64(&json!(40.5)), Some(40.5));
    assert_eq!(value_as_f64(&json!("north")), None);
}
