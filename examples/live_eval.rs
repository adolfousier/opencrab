//! On-demand live eval runner (umbrella #628). Resolves `agent.eval_providers`
//! into real providers, builds a majority-vote judge panel, produces real
//! artifacts, and grades them. Makes real API calls — run only on demand:
//!
//!   cargo run --example live_eval --features eval
//!
//! Never part of CI.

use std::sync::Arc;

use opencrabs::brain::prompt_builder::{BrainLoader, RuntimeInfo};
use opencrabs::brain::provider::Provider;
use opencrabs::config::Config;
use opencrabs::eval::compaction::CompactionDataset;
use opencrabs::eval::live::resolve_eval_providers;
use opencrabs::eval::panel::panel_from_providers;
use opencrabs::eval::produce::{produce_compaction_summary, produce_response};
use opencrabs::eval::runner::VarianceReport;
use opencrabs::eval::self_awareness::SelfAwarenessScenario;

#[tokio::main]
async fn main() {
    let config = Config::load().expect("load config");
    let providers = resolve_eval_providers(&config).await;
    println!("Resolved {} eval provider(s):", providers.len());
    for p in &providers {
        println!("  - {} / {}", p.name(), p.default_model());
    }
    if providers.is_empty() {
        eprintln!("No eval providers resolved — check agent.eval_providers and keys.toml.");
        return;
    }

    // Producer = first provider (the "model under test"); judges = full panel.
    let producer: Arc<dyn Provider> = providers[0].clone();
    let producer_model = producer.default_model().to_string();
    let panel = panel_from_providers(&providers);

    // Build the REAL OpenCrabs system brain so the producer answers with its
    // actual runtime context (compiled-features line + SELF-AWARENESS / VOICE
    // directives), not a bare prompt — the whole point of measuring
    // OpenCrabs-with-the-preamble rather than the raw model.
    let loader = BrainLoader::new(opencrabs::config::opencrabs_home());
    let rt = RuntimeInfo {
        model: Some(producer_model.clone()),
        provider: Some(producer.name().to_string()),
        working_directory: None,
    };
    let system_brain = loader.build_core_brain(Some(&rt));
    let sys = Some(system_brain.as_str());
    println!(
        "\nProducer: {} / {}   Judge panel: {} model(s)   System brain: {} chars\n",
        producer.name(),
        producer_model,
        panel.len(),
        system_brain.len()
    );

    // Repeat each eval K times — a single live run is noise (the producer is
    // non-deterministic), so we report the mean/variance of the overall scores.
    const K: usize = 5;

    // 1. Compaction fidelity.
    let ds = CompactionDataset::seed();
    println!("== Compaction fidelity ({}), {K} runs ==", ds.name);
    let (mut kw, mut pn) = (Vec::new(), Vec::new());
    for i in 0..K {
        let summary =
            produce_compaction_summary(producer.as_ref(), &producer_model, &ds.messages(), sys)
                .await;
        let (k, p) = (
            ds.keyword_scorecard(&summary).overall(),
            ds.judge_scorecard(&panel, &summary).await.overall(),
        );
        println!(
            "  run {i}: {} chars  keyword={k:.2}  panel={p:.2}",
            summary.len()
        );
        kw.push(k);
        pn.push(p);
    }
    println!("  keyword {}", VarianceReport::from_scores(&kw).render());
    println!("  panel   {}", VarianceReport::from_scores(&pn).render());

    // 2. Capability self-awareness — produced UNDER the real system brain.
    let sc = SelfAwarenessScenario::seed();
    println!("\n== Capability self-awareness ({}), {K} runs ==", sc.name);
    let (mut kw, mut pn) = (Vec::new(), Vec::new());
    for i in 0..K {
        let response = produce_response(producer.as_ref(), &producer_model, &sc.prompt, sys).await;
        let (k, p) = (
            sc.keyword_scorecard(&response).overall(),
            sc.judge_scorecard(&panel, &response).await.overall(),
        );
        println!(
            "  run {i}: {} chars  keyword={k:.2}  panel={p:.2}",
            response.len()
        );
        kw.push(k);
        pn.push(p);
    }
    println!("  keyword {}", VarianceReport::from_scores(&kw).render());
    println!("  panel   {}", VarianceReport::from_scores(&pn).render());
}
