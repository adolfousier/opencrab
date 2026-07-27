//! Persisted timestamps must state their zone (#826).
//!
//! Two shapes were in use that look identical and are not:
//! `Local::now().format("%Y-%m-%dT%H:%M:%S")` and
//! `Utc::now().format("%Y-%m-%dT%H:%M:%SZ")`. The first carries no zone, so
//! memory documents stored a local time beside UTC epochs everywhere else and
//! it read back as though it were UTC.
//!
//! The cost is not theoretical. Correlating a chat timestamp against DB rows
//! without applying the offset pulled records from an hour later and produced
//! two confident, evidence-backed, wrong diagnoses in a single investigation
//! before the skew was noticed.

use crate::utils::string::utc_timestamp;

#[test]
fn the_zone_is_explicit() {
    // The whole point: a reader must never have to guess.
    let ts = utc_timestamp();
    assert!(ts.ends_with('Z'), "timestamp must state its zone: {ts}");
}

#[test]
fn it_parses_as_a_real_utc_instant() {
    let ts = utc_timestamp();
    let parsed = chrono::DateTime::parse_from_rfc3339(&ts)
        .unwrap_or_else(|e| panic!("must be valid RFC 3339: {ts} ({e})"));
    assert_eq!(parsed.offset().local_minus_utc(), 0, "must be UTC: {ts}");
}

#[test]
fn it_is_utc_not_local() {
    // The actual defect: a local clock rendered without a zone. Comparing
    // against Utc::now catches a host whose local time is offset, which is
    // where the hour of skew came from.
    let ts = utc_timestamp();
    let now = chrono::Utc::now();
    let parsed = chrono::DateTime::parse_from_rfc3339(&ts).unwrap().to_utc();
    let skew = (now - parsed).num_seconds().abs();
    assert!(skew < 5, "expected UTC, got {skew}s of skew: {ts}");
}

#[test]
fn it_sorts_lexicographically_in_time_order() {
    // These are stored and compared as strings, so string order has to match
    // chronological order or "most recent" silently picks the wrong row.
    let earlier = "2026-07-26T22:00:00Z";
    let later = "2026-07-26T23:00:00Z";
    assert!(earlier < later);
    // And across a date boundary, where a looser format would break.
    assert!("2026-07-31T23:59:00Z" < "2026-08-01T00:01:00Z");
}
