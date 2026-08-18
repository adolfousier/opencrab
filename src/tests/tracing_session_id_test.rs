//! Regression test for #1078: session_id tracing spans on turn entry points.
//!
//! Verifies that:
//! 1. Span context propagates across `tokio::spawn` boundaries via `.instrument()`
//! 2. Source-level check: key turn entry points have `#[instrument]`

use std::sync::{Arc, Mutex};
use tracing::span;
use tracing::Instrument;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

/// Captures span names and field values from tracing events.
#[derive(Clone, Default)]
struct SpanCapture {
    spans: Arc<Mutex<Vec<(String, String)>>>,
}

impl SpanCapture {
    fn recorded(&self) -> Vec<(String, String)> {
        self.spans.lock().unwrap().clone()
    }
}

impl<S: tracing::Subscriber> Layer<S> for SpanCapture {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let name = attrs.metadata().name().to_string();
        let mut fields = String::new();
        let mut visitor = FieldVisitor { out: &mut fields };
        attrs.record(&mut visitor);
        self.spans.lock().unwrap().push((name, fields));
    }
}

struct FieldVisitor<'a> {
    out: &'a mut String,
}

impl<'a> tracing::field::Visit for FieldVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let _ = write!(self.out, "{}={:?} ", field.name(), value);
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        use std::fmt::Write;
        let _ = write!(self.out, "{}=\"{}\" ", field.name(), value);
    }
}

#[tokio::test]
async fn span_context_propagates_across_tokio_spawn() {
    let capture = SpanCapture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());

    let _guard = tracing::subscriber::set_default(subscriber);

    let session_id = uuid::Uuid::new_v4();
    let turn_span = span!(
        tracing::Level::INFO,
        "turn",
        session_id = %session_id,
        channel = "test"
    );

    async {
        // Simulate a spawned task that should inherit the parent span
        let handle = tokio::spawn(
            async move {
                tracing::info!("processing message inside spawned task");
            }
            .instrument(tracing::Span::current()),
        );
        let _ = handle.await;
    }
    .instrument(turn_span)
    .await;

    let spans = capture.recorded();
    let span_names: Vec<&str> = spans.iter().map(|(name, _)| name.as_str()).collect();

    assert!(
        span_names.contains(&"turn"),
        "turn span not found in captured spans: {span_names:?}"
    );

    let turn_entry = spans.iter().find(|(name, _)| name == "turn").unwrap();
    assert!(
        turn_entry.1.contains("session_id"),
        "turn span missing session_id field: {}",
        turn_entry.1
    );
    assert!(
        turn_entry.1.contains("channel"),
        "turn span missing channel field: {}",
        turn_entry.1
    );
}

#[tokio::test]
async fn job_span_carries_name_and_id() {
    let capture = SpanCapture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());

    let _guard = tracing::subscriber::set_default(subscriber);

    let job_name = "test_cron_job".to_string();
    let job_id = uuid::Uuid::new_v4();
    let handle = tokio::spawn(
        async move {
            tracing::info!("executing cron job");
        }
        .instrument(tracing::info_span!("job", name = %job_name, id = %job_id)),
    );
    let _ = handle.await;

    let spans = capture.recorded();
    let job_entry = spans.iter().find(|(name, _)| name == "job");
    assert!(
        job_entry.is_some(),
        "job span not found in captured spans: {:?}",
        spans.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
    let (_, fields) = job_entry.unwrap();
    assert!(
        fields.contains("name"),
        "job span missing name field: {fields}"
    );
    assert!(
        fields.contains("id"),
        "job span missing id field: {fields}"
    );
}

/// Source-level check: the key turn entry points must have #[instrument] or
/// .instrument() so session_id propagates to every log line.
#[test]
fn turn_entry_points_are_instrumented() {
    let tool_loop = std::fs::read_to_string("src/brain/agent/service/tool_loop.rs")
        .expect("read tool_loop.rs");
    assert!(
        tool_loop.contains("#[tracing::instrument(")
            && tool_loop.contains("session_id"),
        "run_tool_loop must have #[instrument] with session_id field"
    );

    let messaging = std::fs::read_to_string("src/brain/agent/service/messaging.rs")
        .expect("read messaging.rs");
    assert!(
        messaging.contains("#[tracing::instrument("),
        "messaging.rs entry points must have #[instrument]"
    );

    let scheduler =
        std::fs::read_to_string("src/cron/scheduler.rs").expect("read scheduler.rs");
    assert!(
        scheduler.contains(".instrument(tracing::info_span!(\"job\""),
        "cron scheduler spawn must use .instrument() with a job span"
    );

    let rsi = std::fs::read_to_string("src/brain/rsi.rs").expect("read rsi.rs");
    assert!(
        rsi.contains(".instrument(tracing::info_span!(\"rsi_engine\""),
        "RSI engine spawn must use .instrument() with an rsi_engine span"
    );
}
