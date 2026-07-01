//! Cron Scheduler
//!
//! Background task that checks the `cron_jobs` table every 60 seconds,
//! executes due jobs in a shared "Cron" session, and delivers results
//! to the configured channel. Each run inserts a compaction marker after
//! completion so the next run starts with empty context (no cross-job
//! history contamination). Cron jobs are fully isolated from the TUI —
//! they never share or mutate the user's active session.

use crate::channels::ChannelFactory;
use crate::config::Config;
use crate::db::CronJobRepository;
use crate::db::CronJobRunRepository;
use crate::db::models::{CronJob, CronJobRun};
use crate::services::{ServiceContext, SessionService};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

/// Whether `job_profile` is the active process profile (so the cheap, already
/// wired factory agent can run it) rather than a foreign profile that needs its
/// own config + brain materialized. `None` = legacy pre-stamping row, treated as
/// the active profile. The base profile is stored as the literal "default".
fn is_active_profile(job_profile: Option<&str>, active: Option<&str>) -> bool {
    match job_profile {
        None => true,
        Some(stamped) => stamped == active.unwrap_or("default"),
    }
}

/// Reserved cron-job name for a one-shot background `/rebuild`. The scheduler
/// special-cases this name: instead of running an agent prompt it builds from
/// source and exec-restarts into the new binary, then the job removes itself.
/// The originating session id is carried in `prompt` so the restart resumes
/// the user's session.
pub const REBUILD_JOB_NAME: &str = "__opencrabs_rebuild__";

/// Schedule a one-shot background rebuild for `session_id`. Returns once the
/// job is queued — the build runs out-of-band on the scheduler's next tick
/// (within ~60s), so the calling session is never blocked. `deliver_to` (if
/// set) receives a status message; the reload resumes `session_id`.
pub async fn schedule_background_rebuild(
    pool: crate::db::Pool,
    session_id: Uuid,
    deliver_to: Option<String>,
) -> anyhow::Result<()> {
    let repo = CronJobRepository::new(pool);
    // Remove any stale rebuild job first so we never stack two builds.
    if let Ok(existing) = repo.list_all().await {
        for j in existing.iter().filter(|j| j.name == REBUILD_JOB_NAME) {
            let _ = repo.delete(&j.id.to_string()).await;
        }
    }
    let now = Utc::now();
    let job = CronJob {
        id: Uuid::new_v4(),
        name: REBUILD_JOB_NAME.to_string(),
        // Every minute → the next tick (within 60s) picks it up; the job
        // deletes itself on pickup so it runs exactly once.
        cron_expr: "* * * * *".to_string(),
        timezone: "UTC".to_string(),
        prompt: session_id.to_string(),
        provider: None,
        model: None,
        thinking: "off".to_string(),
        auto_approve: true,
        deliver_to,
        deliver_api_key: None,
        enabled: true,
        last_run_at: None,
        next_run_at: None,
        created_at: now,
        updated_at: now,
        // Stamp the current profile so the guard in `tick()` lets it run here.
        // current_profile_name() honors the task-local profile scope.
        profile_name: Some(crate::config::profile::current_profile_name()),
    };
    repo.insert(&job).await?;
    tracing::info!("Background rebuild queued for session {session_id}");
    Ok(())
}

/// Execute the reserved background-rebuild job: delete it first (one-shot, no
/// retry on the 60s tick), build from source, then exec-restart into the
/// freshly-built binary (replaces the whole process). On failure it reports
/// to `deliver_to` and returns. The originating session id is in `job.prompt`.
async fn run_rebuild_job(job: &CronJob, ctx: &ServiceContext) -> anyhow::Result<()> {
    use crate::brain::SelfUpdater;

    // Delete up front so a long/failed build can't re-trigger next tick.
    let repo = CronJobRepository::new(ctx.pool());
    if let Err(e) = repo.delete(&job.id.to_string()).await {
        tracing::error!("rebuild job: failed to delete self: {e}");
    }

    let session_id = Uuid::parse_str(job.prompt.trim()).unwrap_or_else(|_| Uuid::nil());
    tracing::info!("Background rebuild starting (will resume session {session_id})");

    let updater =
        SelfUpdater::auto_detect().map_err(|e| anyhow::anyhow!("rebuild: auto_detect: {e}"))?;

    match updater
        .build_streaming(|line| tracing::debug!("rebuild: {line}"))
        .await
    {
        Ok(built_path) => {
            tracing::info!(
                "Background rebuild succeeded: {} — reloading",
                built_path.display()
            );
            deliver_rebuild_status(
                job,
                "✅ Rebuilt from source — reloading into the new binary now.",
            )
            .await;
            // exec() replaces the entire process (this scheduler task too).
            if let Err(e) = SelfUpdater::restart_into(&built_path, session_id) {
                tracing::error!("Background rebuild restart failed: {e}");
                return Err(anyhow::anyhow!("rebuild restart failed: {e}"));
            }
            Ok(()) // unreachable on success
        }
        Err(out) => {
            tracing::error!("Background rebuild failed: {out}");
            deliver_rebuild_status(job, &format!("⚠️ Background rebuild failed:\n{out}")).await;
            Ok(())
        }
    }
}

/// Deliver a rebuild status line to the job's configured channels (if any).
async fn deliver_rebuild_status(job: &CronJob, msg: &str) {
    if let Some(ref deliver_to) = job.deliver_to {
        for target in deliver_to
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            // Rebuild status messages aren't worth reply recovery — no pool.
            deliver_result(target, &job.name, msg, job.deliver_api_key.as_deref(), None).await;
        }
    }
}

/// Background cron scheduler that polls the database and executes due jobs.
pub struct CronScheduler {
    repo: CronJobRepository,
    run_repo: CronJobRunRepository,
    factory: Arc<ChannelFactory>,
    service_context: ServiceContext,
}

impl CronScheduler {
    pub fn new(
        repo: CronJobRepository,
        run_repo: CronJobRunRepository,
        factory: Arc<ChannelFactory>,
        service_context: ServiceContext,
    ) -> Self {
        Self {
            repo,
            run_repo,
            factory,
            service_context,
        }
    }

    /// Spawn the scheduler as a background tokio task.
    /// Polls every 60 seconds for due jobs.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    /// Run the polling loop in the CURRENT task (no internal spawn). The
    /// multi-profile daemon drives this directly inside a
    /// `with_profile_home_async(profile, ...)` scope so the scheduler's own
    /// setup (cron session, config reads) resolves to that profile's home.
    /// `spawn()` is the thin wrapper for callers that just want it backgrounded.
    pub async fn run(self) {
        tracing::info!(
            "Cron scheduler started — polling every 60s (shared Cron session, compaction-isolated)"
        );
        loop {
            if let Err(e) = self.tick().await {
                tracing::error!("Cron scheduler tick error: {e}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    }

    /// One scheduler tick: check all enabled jobs and execute any that are due.
    async fn tick(&self) -> anyhow::Result<()> {
        let jobs = self.repo.list_enabled().await?;
        let now = Utc::now();

        for job in &jobs {
            if self.is_due(job, now) {
                tracing::info!("Cron job '{}' ({}) is due — executing", job.name, job.id);

                // Calculate next run time before executing (so we don't re-trigger).
                // Use the scheduled boundary as anchor, not `now`: for a
                // first-run job (next_run_at = None), `now` may be 16s before
                // the boundary (e.g. 10:59:44 vs 11:00:00). Passing `now`
                // resolves to the SAME boundary, causing a double-fire next tick.
                // (#224)
                let next_run = match job.next_run_at {
                    Some(scheduled) => self.next_run_after(job, scheduled),
                    None => match super::next_run_utc(&job.cron_expr, job_tz(job), now) {
                        Some(boundary) => self.next_run_after(job, boundary),
                        None => None,
                    },
                };
                let next_run_str = next_run.map(|dt| dt.to_rfc3339());
                self.repo
                    .update_last_run(&job.id.to_string(), next_run_str.as_deref())
                    .await?;

                // Execute in background so we don't block other jobs
                let job = job.clone();
                let factory = self.factory.clone();
                let ctx = self.service_context.clone();
                let run_repo = self.run_repo.clone();
                tokio::spawn(async move {
                    // For foreign-profile jobs, wrap the ENTIRE execution in a
                    // task-local profile home scope. This means every tool call
                    // the agent makes (memory writes, config reads, file ops,
                    // brain reads) resolves to the job's profile home, not the
                    // process profile. The scope lives until the task ends, so
                    // it persists across every .await inside the agent loop.
                    //
                    // This spawned task does NOT inherit the scheduler's own
                    // task-local home (tokio::spawn drops it), so it defaults to
                    // the process global. We therefore scope whenever the job's
                    // profile differs from the process global, which is exactly
                    // the multi-profile daemon case: a per-profile scheduler's
                    // jobs are stamped with a non-global profile and get scoped
                    // here.
                    let profile = job.profile_name.as_deref();
                    let active = crate::config::profile::active_profile().unwrap_or("default");
                    let needs_scope = profile.is_some() && profile != Some(active);

                    let result = if needs_scope {
                        crate::config::profile::with_profile_home_async(profile, async {
                            tracing::info!(
                                "Cron job '{}' — task-local profile home set to {:?}",
                                job.name,
                                crate::config::opencrabs_home()
                            );
                            match resolve_or_create_cron_session(&ctx).await {
                                Ok(cron_sid) => {
                                    execute_job(&job, &factory, &ctx, cron_sid, &run_repo).await
                                }
                                Err(e) => Err(e),
                            }
                        })
                        .await
                    } else {
                        match resolve_or_create_cron_session(&ctx).await {
                            Ok(cron_sid) => {
                                execute_job(&job, &factory, &ctx, cron_sid, &run_repo).await
                            }
                            Err(e) => Err(e),
                        }
                    };

                    if let Err(e) = result {
                        tracing::error!("Cron job '{}' failed: {e}", job.name);
                    }
                });
            }
        }

        Ok(())
    }

    /// Check if a job is due to run.
    fn is_due(&self, job: &CronJob, now: chrono::DateTime<Utc>) -> bool {
        match &job.next_run_at {
            // If next_run_at is set and is in the past (or now), it's due
            Some(next) => *next <= now,
            // If next_run_at is None (first run), calculate from cron and check
            None => {
                // Interpret the schedule in the job's timezone (DST-aware),
                // then compare the resulting UTC instant. If any upcoming run
                // is within the next 60s (one tick), it's due.
                match super::next_run_utc(&job.cron_expr, job_tz(job), now) {
                    Some(next) => (next - now).num_seconds() <= 60,
                    None => {
                        tracing::warn!(
                            "Invalid cron expression for job '{}': {}",
                            job.name,
                            job.cron_expr
                        );
                        false
                    }
                }
            }
        }
    }

    /// Calculate the next run time after a given point, in the job's timezone.
    fn next_run_after(
        &self,
        job: &CronJob,
        after: chrono::DateTime<Utc>,
    ) -> Option<chrono::DateTime<Utc>> {
        super::next_run_utc(&job.cron_expr, job_tz(job), after)
    }
}

/// Find an existing "Cron" session or create one. All cron jobs share this
/// session for logging/debugging, but each run inserts a compaction marker
/// after completion so the next run starts with empty context (no history
/// contamination between jobs).
async fn resolve_or_create_cron_session(ctx: &ServiceContext) -> anyhow::Result<Uuid> {
    const CRON_SESSION_NAME: &str = "Cron";
    use crate::db::repository::SessionListOptions;
    let session_svc = SessionService::new(ctx.clone());
    let sessions = session_svc
        .list_sessions(SessionListOptions {
            include_archived: false,
            limit: None,
            offset: 0,
            query: None,
        })
        .await?;
    if let Some(existing) = sessions
        .iter()
        .find(|s| s.title.as_deref().is_some_and(|n| n == CRON_SESSION_NAME))
    {
        return Ok(existing.id);
    }
    let config = Config::load()?;
    let provider = config.cron.default_provider.clone();
    let model = config.cron.default_model.clone();
    let session = session_svc
        .create_session_with_provider(Some(CRON_SESSION_NAME.to_string()), provider, model, None)
        .await?;
    Ok(session.id)
}

/// Resolve a job's stored timezone string to a `Tz`, falling back to UTC for
/// an unknown zone (the tool/CLI reject unknown zones at creation, so this is
/// just a safety net for hand-edited rows).
fn job_tz(job: &CronJob) -> chrono_tz::Tz {
    super::parse_timezone(&job.timezone).unwrap_or(chrono_tz::UTC)
}

/// Resolve the `(Config, AgentService)` a job should run with.
///
/// Jobs created in the active profile (and legacy unstamped jobs) use the
/// already-wired factory agent. A job stamped with a DIFFERENT profile
/// (shared-DB case, #182) gets its own config + brain + provider built from
/// that profile's home. The shared DB pool is reused since that's exactly why
/// the foreign job is visible to this scheduler at all.
async fn resolve_job_agent(
    job: &CronJob,
    factory: &ChannelFactory,
    ctx: &ServiceContext,
) -> anyhow::Result<(Config, Arc<crate::brain::agent::AgentService>)> {
    // Use the task-local profile (set by the per-job home scope above) rather
    // than the process global, so a job running under its own profile scope
    // is recognized as "local" and reuses the in-scope factory/config instead
    // of needlessly re-materializing. Falls back to the global when unscoped.
    let current = crate::config::profile::current_profile_name();
    if is_active_profile(job.profile_name.as_deref(), Some(&current)) {
        return Ok((Config::load()?, factory.create_agent_service().await));
    }

    let profile = job.profile_name.as_deref();
    tracing::info!(
        "Cron job '{}' belongs to profile {:?} (current profile {:?}); \
         running under its own profile context",
        job.name,
        profile,
        current
    );

    // Materialize config + brain from the foreign profile's home.
    // with_profile_home sets a sync scope just for these two loads.
    let (config, brain, home) = crate::config::profile::with_profile_home(profile, || {
        let config = Config::load()?;
        let home = crate::config::opencrabs_home();
        let brain =
            crate::brain::prompt_builder::BrainLoader::new(home.clone()).build_core_brain(None);
        anyhow::Ok((config, brain, home))
    })?;

    // Provider is built from the foreign profile's keys.
    let provider = crate::brain::provider::create_provider(&config).await?;
    let mut builder = crate::brain::agent::AgentService::new(provider, ctx.clone(), &config)
        .await
        .with_system_brain(brain)
        .with_working_directory(home.clone())
        .with_brain_path(home);
    if let Some(registry) = factory.tool_registry() {
        builder = builder.with_tool_registry(registry);
    }
    Ok((config, Arc::new(builder)))
}

/// Execute a single cron job in its own isolated session.
/// Isolated from TUI — never touches the user's active session.
/// Results are always stored in the DB; channel delivery is optional.
async fn execute_job(
    job: &CronJob,
    factory: &ChannelFactory,
    ctx: &ServiceContext,
    cron_session_id: Uuid,
    run_repo: &CronJobRunRepository,
) -> anyhow::Result<()> {
    // Reserved one-shot background rebuild — build + exec-restart, never an
    // agent prompt.
    if job.name == REBUILD_JOB_NAME {
        return run_rebuild_job(job, ctx).await;
    }

    // Resolve the config + agent for this job's profile. A job created in a
    // non-active profile (shared-DB case, #182) runs under its own profile's
    // config + brain, not the process profile's.
    let (config, agent) = resolve_job_agent(job, factory, ctx).await?;
    let effective_provider = job
        .provider
        .clone()
        .or_else(|| config.cron.default_provider.clone());
    let effective_model = job
        .model
        .clone()
        .or_else(|| config.cron.default_model.clone());

    // Pre-validate the {provider, model} pair before spawning the agent.
    // A reversed cron config (e.g. model="zhipu", provider="glm-5.1") or a
    // typo would otherwise reach the tool loop and produce confusing RSI
    // entries like "dialagram/zhipu" where a provider name leaked into the
    // model slot. Catch it early, log loudly, skip the job.
    if let Some(ref provider_name) = effective_provider
        && let Some(ref model) = effective_model
    {
        match crate::brain::provider::create_provider_by_name(&config, provider_name).await {
            Ok(provider) => {
                let supported = provider.supported_models();
                if !supported.is_empty() && !supported.iter().any(|m| m == model) {
                    tracing::error!(
                        "Cron job '{}' — model '{}' is NOT supported by provider '{}' \
                         (supported: {}). SKIPPING job — fix cron config. \
                         Either set a valid model or remove the model override to use \
                         the provider's default ('{}').",
                        job.name,
                        model,
                        provider_name,
                        supported.join(", "),
                        provider.default_model(),
                    );
                    // Record the failure so RSI surfaces it
                    let run = CronJobRun::new_running(
                        job.id,
                        job.name.clone(),
                        effective_provider.clone(),
                        effective_model.clone(),
                    );
                    let run_id = run.id.to_string();
                    if let Err(e) = run_repo.insert(&run).await {
                        tracing::error!("Failed to insert cron run record: {e}");
                    }
                    let err_msg = format!(
                        "model '{}' not supported by provider '{}' — cron config invalid",
                        model, provider_name
                    );
                    if let Err(db_err) = run_repo.complete_error(&run_id, &err_msg).await {
                        tracing::error!("Failed to save cron run error to DB: {db_err}");
                    }
                    return Ok(());
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Cron job '{}' — cannot pre-validate model (provider '{}' creation \
                     failed: {e}) — proceeding with default validation",
                    job.name,
                    provider_name
                );
            }
        }
    }

    // Create a run record in the DB (status = "running")
    let run = CronJobRun::new_running(
        job.id,
        job.name.clone(),
        effective_provider.clone(),
        effective_model.clone(),
    );
    let run_id = run.id.to_string();
    if let Err(e) = run_repo.insert(&run).await {
        tracing::error!("Failed to insert cron run record: {e}");
    }

    let session_id = cron_session_id;
    tracing::info!(
        "Cron job '{}' — using cron session {}",
        job.name,
        session_id
    );

    // Swap to cron-specific provider if configured
    if let Some(ref provider_name) = effective_provider {
        match crate::brain::provider::create_provider_by_name(&config, provider_name).await {
            Ok(provider) => {
                tracing::info!(
                    "Cron job '{}' — using provider '{}'",
                    job.name,
                    provider_name
                );
                agent.swap_provider_for_session(
                    cron_session_id,
                    provider.clone(),
                    provider.default_model().to_string(),
                );
            }
            Err(e) => {
                tracing::warn!(
                    "Cron job '{}' — failed to create provider '{}': {e}, using system default",
                    job.name,
                    provider_name
                );
            }
        }
    }

    // Execute with auto-approved tools (no interactive user)
    let result = agent
        .send_message_with_tools_and_callback(
            session_id,
            job.prompt.clone(),
            effective_model,
            None, // no cancel token
            Some(Arc::new(|_| {
                // Auto-approve all tools for cron jobs
                Box::pin(async { Ok((true, false)) })
            })),
            None, // no progress callback
            None, // no follow_up_question surface for cron
            "cron",
            None,
        )
        .await;

    match result {
        Ok(response) => {
            let clean = crate::utils::sanitize::strip_llm_artifacts(&response.content);

            tracing::info!(
                "Cron job '{}' completed — {} tokens, ${:.6}",
                job.name,
                response.usage.input_tokens + response.usage.output_tokens,
                response.cost
            );

            // Save result to DB
            if let Err(e) = run_repo
                .complete_success(
                    &run_id,
                    &clean,
                    response.usage.input_tokens as i64,
                    response.usage.output_tokens as i64,
                    response.cost,
                )
                .await
            {
                tracing::error!("Failed to save cron run result to DB: {e}");
            }

            // Optionally deliver to configured channels too
            if let Some(ref deliver_to) = job.deliver_to {
                for target in deliver_to
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    deliver_result(
                        target,
                        &job.name,
                        &clean,
                        job.deliver_api_key.as_deref(),
                        Some(ctx.pool()),
                    )
                    .await;
                }
            }
        }
        Err(e) => {
            tracing::error!("Cron job '{}' agent error: {e}", job.name);

            // Save error to DB
            let error_msg = format!("{e}");
            if let Err(db_err) = run_repo.complete_error(&run_id, &error_msg).await {
                tracing::error!("Failed to save cron run error to DB: {db_err}");
            }

            // Optionally deliver error to configured channels too
            if let Some(ref deliver_to) = job.deliver_to {
                let msg = format!("Cron job '{}' failed: {e}", job.name);
                for target in deliver_to
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    deliver_result(
                        target,
                        &job.name,
                        &msg,
                        job.deliver_api_key.as_deref(),
                        Some(ctx.pool()),
                    )
                    .await;
                }
            }
        }
    }

    // Insert a compaction marker so the next cron run starts with empty
    // context. Without this, every job would see the full conversation
    // history of all previous jobs (the contamination vector).
    let message_svc = crate::services::MessageService::new(ctx.clone());
    if let Err(e) = message_svc
        .create_message(
            session_id,
            "user".to_string(),
            "[CONTEXT COMPACTION — Cron job execution boundary]".to_string(),
        )
        .await
    {
        tracing::warn!("Failed to insert cron compaction marker: {e}");
    }

    Ok(())
}

/// Deliver a cron job result to the specified channel.
/// Format: "telegram:chat_id", "discord:channel_id", "slack:channel_id",
/// or an HTTP(S) URL for generic webhook delivery.
async fn deliver_result(
    deliver_to: &str,
    job_name: &str,
    content: &str,
    api_key: Option<&str>,
    pool: Option<crate::db::Pool>,
) {
    // Only the Telegram delivery arm uses the pool (to record the message for
    // reply recovery); other targets ignore it.
    #[cfg(not(feature = "telegram"))]
    let _ = &pool;
    // HTTP(S) URL — generic webhook delivery
    if deliver_to.starts_with("http://") || deliver_to.starts_with("https://") {
        deliver_http(deliver_to, job_name, content, api_key).await;
        return;
    }

    let parts: Vec<&str> = deliver_to.splitn(2, ':').collect();
    if parts.len() != 2 {
        tracing::warn!(
            "Invalid deliver_to format '{}' for job '{}' — expected 'channel:id' or HTTP URL",
            deliver_to,
            job_name
        );
        return;
    }

    let (channel, target_id) = (parts[0], parts[1]);

    // Truncate content for delivery (channels have message limits)
    let max_len = 4000;
    let msg = if content.len() > max_len {
        format!(
            "{}...\n\n(truncated — full output in session)",
            &content[..max_len]
        )
    } else {
        content.to_string()
    };

    let delivery_msg = format!("⏰ **Cron: {job_name}**\n\n{msg}");

    match channel {
        "telegram" => {
            #[cfg(feature = "telegram")]
            {
                tracing::info!("Delivering cron result to Telegram chat {target_id}");
                deliver_telegram(target_id, &delivery_msg, pool.clone()).await;
            }
            #[cfg(not(feature = "telegram"))]
            {
                tracing::warn!("Telegram feature not enabled — cannot deliver cron result");
            }
        }
        "discord" => {
            #[cfg(feature = "discord")]
            {
                tracing::info!("Delivering cron result to Discord channel {target_id}");
                deliver_discord(target_id, &delivery_msg).await;
            }
            #[cfg(not(feature = "discord"))]
            {
                tracing::warn!("Discord feature not enabled — cannot deliver cron result");
            }
        }
        "slack" => {
            #[cfg(feature = "slack")]
            {
                tracing::info!("Delivering cron result to Slack channel {target_id}");
                deliver_slack(target_id, &delivery_msg).await;
            }
            #[cfg(not(feature = "slack"))]
            {
                tracing::warn!("Slack feature not enabled — cannot deliver cron result");
            }
        }
        other => {
            tracing::warn!("Unknown delivery channel '{other}' for job '{job_name}'");
        }
    }
}

/// Deliver cron result via HTTP POST to a generic webhook URL.
async fn deliver_http(url: &str, job_name: &str, content: &str, api_key: Option<&str>) {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "job_name": job_name,
        "content": content,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    let mut request = client.post(url).json(&body);

    // Attach Bearer token if the job has one configured.
    if let Some(key) = api_key {
        request = request.header("Authorization", format!("Bearer {key}"));
    }

    match request.send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!("Cron result for '{job_name}' delivered to {url}");
        }
        Ok(resp) => {
            tracing::warn!(
                "HTTP delivery to {url} failed ({}): {:?}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
        }
        Err(e) => {
            tracing::error!("HTTP delivery to {url} error: {e}");
        }
    }
}

/// Read `channels.<channel>.<field>` (e.g. a bot token) from the active
/// workspace's `keys.toml`. Cron delivery runs outside any channel's live
/// connection, so it reads the credential straight off disk.
#[cfg(any(feature = "telegram", feature = "discord", feature = "slack"))]
fn read_channel_secret(channel: &str, field: &str) -> Option<String> {
    let keys_path = crate::brain::BrainLoader::resolve_path().join("keys.toml");
    let content = std::fs::read_to_string(&keys_path).ok()?;
    content.parse::<toml::Table>().ok().and_then(|t| {
        t.get("channels")?
            .as_table()?
            .get(channel)?
            .as_table()?
            .get(field)?
            .as_str()
            .map(String::from)
    })
}

/// Split `text` into `<= max_len` byte chunks, breaking on a newline near the
/// limit when possible and never inside a multi-byte char. Used for Discord's
/// 2000-char and Slack's message limits. (Telegram reuses its own chunker so
/// HTML stays valid across splits.)
#[cfg(any(feature = "discord", feature = "slack"))]
fn split_for_delivery(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + max_len).min(text.len());
        while end < text.len() && !text.is_char_boundary(end) {
            end -= 1;
        }
        let break_at = if end < text.len() {
            text[start..end]
                .rfind('\n')
                .filter(|&pos| pos > end - start - 200)
                .map(|pos| start + pos + 1)
                .unwrap_or(end)
        } else {
            end
        };
        chunks.push(&text[start..break_at]);
        start = break_at;
    }
    chunks
}

/// Deliver via Telegram Bot API (direct HTTP POST).
#[cfg(feature = "telegram")]
async fn deliver_telegram(chat_id: &str, message: &str, pool: Option<crate::db::Pool>) {
    let Some(token) = read_channel_secret("telegram", "token") else {
        tracing::warn!("No Telegram bot token found in keys.toml — cannot deliver cron result");
        return;
    };

    use crate::channels::telegram::handler::{markdown_to_telegram_html, split_message};
    use crate::channels::telegram::rich;

    // Telegram message_id of the delivered cron message, captured so a user who
    // replies to it can have its content recovered exactly by id (cron messages
    // were never stored before, so replying to one surfaced no context — the
    // reply handler then guessed the wrong message, #234 follow-up).
    let mut sent_id: Option<i32> = None;

    // Render exactly like an interactive Telegram reply — never the fragile
    // legacy `parse_mode: "Markdown"`, which breaks on '_', '[', etc. and
    // routinely 400s. Honor the `channels.telegram.rich_messages` flag: when
    // it's on AND the content has block structure (tables/headings/lists/math),
    // deliver a native rich message through our parser; otherwise (and on any
    // rich failure) fall back to the universal HTML rendering.
    let chat_id_num = chat_id.parse::<i64>().ok();
    let mut delivered_rich = false;
    if let Some(cid) = chat_id_num
        && rich::should_send_native_rich(message)
    {
        match rich::api::send_rich_markdown_id(&token, cid, None, message).await {
            Ok(id) => {
                tracing::info!("Cron result delivered to Telegram chat {chat_id} (native rich)");
                sent_id = Some(id);
                delivered_rich = true;
            }
            Err(e) => {
                tracing::warn!(
                    "Cron native-rich delivery to {chat_id} failed ({e}) — falling back to HTML"
                );
            }
        }
    }

    if !delivered_rich {
        let html = markdown_to_telegram_html(message);
        let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
        let client = reqwest::Client::new();
        let mut delivered = 0usize;
        for chunk in split_message(&html, 4096) {
            let body = serde_json::json!({
                "chat_id": chat_id,
                "text": chunk,
                "parse_mode": "HTML",
            });
            match client.post(&url).json(&body).send().await {
                Ok(resp) if resp.status().is_success() => {
                    delivered += 1;
                    // Capture the message_id of the last chunk — that's the
                    // bubble a user would reply to.
                    if let Ok(json) = resp.json::<serde_json::Value>().await
                        && let Some(id) = json
                            .get("result")
                            .and_then(|r| r.get("message_id"))
                            .and_then(|v| v.as_i64())
                    {
                        sent_id = Some(id as i32);
                    }
                }
                Ok(resp) => {
                    tracing::warn!(
                        "Telegram delivery to {chat_id} failed ({}): {:?}",
                        resp.status(),
                        resp.text().await.unwrap_or_default()
                    );
                }
                Err(e) => {
                    tracing::error!("Telegram delivery to {chat_id} HTTP error: {e}");
                }
            }
        }
        if delivered > 0 {
            tracing::info!(
                "Cron result delivered to Telegram chat {chat_id} (HTML, {delivered} part(s))"
            );
        }
    }

    // Persist the delivered message keyed by its Telegram id so a later reply
    // resolves to this exact content instead of falling back to a wrong guess.
    if let (Some(id), Some(pool)) = (sent_id, pool) {
        let repo = crate::db::ChannelMessageRepository::new(pool);
        let cm = crate::db::models::ChannelMessage::new(
            "telegram".to_string(),
            chat_id.to_string(),
            None,
            "bot:opencrabs".to_string(),
            "OpenCrabs".to_string(),
            message.to_string(),
            "text".to_string(),
            Some(id.to_string()),
        );
        if let Err(e) = repo.insert(&cm).await {
            tracing::warn!("Cron: failed to record delivered message for reply recovery: {e}");
        }
    }
}

/// Deliver via Discord Bot API (direct HTTP POST to the channel-messages
/// endpoint). Discord renders its own markdown natively, so the content is
/// sent as-is, chunked to the 2000-char message limit.
#[cfg(feature = "discord")]
async fn deliver_discord(channel_id: &str, message: &str) {
    let Some(token) = read_channel_secret("discord", "token") else {
        tracing::warn!("No Discord bot token found in keys.toml — cannot deliver cron result");
        return;
    };

    let url = format!("https://discord.com/api/v10/channels/{channel_id}/messages");
    let client = reqwest::Client::new();
    let mut delivered = 0usize;
    for chunk in split_for_delivery(message, 2000) {
        let body = serde_json::json!({ "content": chunk });
        match client
            .post(&url)
            .header("Authorization", format!("Bot {token}"))
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => delivered += 1,
            Ok(resp) => {
                tracing::warn!(
                    "Discord delivery to {channel_id} failed ({}): {:?}",
                    resp.status(),
                    resp.text().await.unwrap_or_default()
                );
            }
            Err(e) => {
                tracing::error!("Discord delivery to {channel_id} HTTP error: {e}");
            }
        }
    }
    if delivered > 0 {
        tracing::info!(
            "Cron result delivered to Discord channel {channel_id} ({delivered} part(s))"
        );
    }
}

/// Deliver via Slack Web API (`chat.postMessage`). The `text` field renders
/// Slack mrkdwn. Slack returns HTTP 200 even on a logical failure
/// (`{"ok":false,"error":...}`), so we inspect the body, not just the status.
#[cfg(feature = "slack")]
async fn deliver_slack(channel_id: &str, message: &str) {
    let Some(token) = read_channel_secret("slack", "token") else {
        tracing::warn!("No Slack bot token found in keys.toml — cannot deliver cron result");
        return;
    };

    let url = "https://slack.com/api/chat.postMessage";
    let client = reqwest::Client::new();
    let mut delivered = 0usize;
    for chunk in split_for_delivery(message, 3500) {
        let body = serde_json::json!({ "channel": channel_id, "text": chunk });
        match client
            .post(url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&body)
            .send()
            .await
        {
            Ok(resp) => {
                let parsed: serde_json::Value = resp.json().await.unwrap_or_default();
                if parsed.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
                    delivered += 1;
                } else {
                    tracing::warn!(
                        "Slack delivery to {channel_id} failed: {}",
                        parsed
                            .get("error")
                            .and_then(|e| e.as_str())
                            .unwrap_or("unknown error")
                    );
                }
            }
            Err(e) => {
                tracing::error!("Slack delivery to {channel_id} HTTP error: {e}");
            }
        }
    }
    if delivered > 0 {
        tracing::info!("Cron result delivered to Slack channel {channel_id} ({delivered} part(s))");
    }
}
