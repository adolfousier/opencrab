//! Cron Manage Tool
//!
//! Allows the agent to create, list, update, delete, enable, and disable cron
//! jobs. Jobs run in isolated sessions with configurable provider/model/thinking.

use super::error::Result;
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::db::models::CronJob;
use crate::db::{CronJobPatch, CronJobRepository};
use async_trait::async_trait;
use serde_json::Value;

/// Tool for managing cron jobs via the agent.
pub struct CronManageTool {
    repo: CronJobRepository,
}

impl CronManageTool {
    pub fn new(repo: CronJobRepository) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl Tool for CronManageTool {
    fn name(&self) -> &str {
        "cron_manage"
    }

    fn description(&self) -> &str {
        "Manage scheduled cron jobs. Jobs run in isolated sessions with configurable provider/model. \
         Use 'create' to schedule a new job, 'list' to see all jobs, 'update' to change fields of an \
         existing job in place (only the fields you pass are touched), 'delete' to remove one, \
         'enable'/'disable' to toggle a job without deleting it."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "list", "delete", "update", "enable", "disable", "test"],
                    "description": "Action to perform. 'test' triggers a job immediately (runs on next scheduler tick within 60s)"
                },
                "name": {
                    "type": "string",
                    "description": "Job name (required for create, optional new name for update)"
                },
                "cron": {
                    "type": "string",
                    "description": "Cron expression, 5-field: 'min hour dom mon dow'. Required for create, optional for update. Day-of-week is 1-7 = Sun-Sat (1=Sunday, 7=Saturday; 0 is INVALID) — prefer day NAMES (Sun, Mon..Sat, ranges like Mon-Fri) to avoid off-by-one mistakes. Month also accepts names (Jan-Mar). No @daily/@hourly macros. Examples: '0 9 * * *' (daily 9am), '*/30 * * * *' (every 30min), '0 9 * * Mon-Fri' (weekdays 9am), '0 22 * * Sun' (Sundays 10pm). The create/update reply shows the next run times — verify them."
                },
                "tz": {
                    "type": "string",
                    "description": "IANA timezone (default: UTC) — the schedule runs in this zone's local wall clock, DST-aware. Examples: America/New_York, Europe/London, Asia/Tokyo. An unknown zone is rejected."
                },
                "prompt": {
                    "type": "string",
                    "description": "Instructions for the agent to execute (required for create, optional for update; omitted on update keeps the existing prompt)"
                },
                "provider": {
                    "type": "string",
                    "description": "Override provider (e.g. 'anthropic', 'openai'). Omit for current default. On update, pass an empty string to clear the override back to the default."
                },
                "model": {
                    "type": "string",
                    "description": "Override model (e.g. 'claude-sonnet-4-20250514'). Omit for provider default. On update, pass an empty string to clear the override."
                },
                "thinking": {
                    "type": "string",
                    "enum": ["off", "on", "budget"],
                    "description": "Thinking mode (default: off)"
                },
                "auto_approve": {
                    "type": "boolean",
                    "description": "Auto-approve tool executions (default: true for cron)"
                },
                "deliver_to": {
                    "type": "string",
                    "description": "Where to deliver results. Format: 'telegram:chat_id', 'telegram:chat_id:thread_id' (opt-in delivery into that forum topic; the chat must be a forum and the topic must exist — invalid thread targets are rejected loudly at fire time, never re-routed to the default topic), 'discord:channel_id', 'slack:channel_id', or an HTTP(S) URL for webhook delivery. On update, pass an empty string to clear delivery."
                },
                "deliver_api_key": {
                    "type": "string",
                    "description": "Optional Bearer token for HTTP webhook delivery. Added as Authorization: Bearer <key> header when delivering to an HTTP(S) URL. On update, pass an empty string to clear it."
                },
                "job_id": {
                    "type": "string",
                    "description": "Job ID or name (required for delete/update/enable/disable)"
                },
                "enabled": {
                    "type": "boolean",
                    "description": "Whether the job is enabled (for create, default: true; for update, sets the enabled state)"
                },
                "confirm": {
                    "type": "boolean",
                    "description": "Must be true to actually delete a job. Without it, delete only shows job details as a safety check."
                }
            },
            "required": ["action"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::SystemModification]
    }

    fn requires_approval_for_input(&self, input: &Value) -> bool {
        // create, update, delete, disable, and test need approval; list/enable are safe
        matches!(
            input.get("action").and_then(|v| v.as_str()),
            Some("create") | Some("update") | Some("delete") | Some("disable") | Some("test")
        )
    }

    async fn execute(&self, input: Value, _context: &ToolExecutionContext) -> Result<ToolResult> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");

        match action {
            "create" => self.create_job(&input).await,
            "list" => self.list_jobs().await,
            "delete" => self.delete_job(&input).await,
            "update" => self.update_job(&input).await,
            "enable" => self.toggle_job(&input, true).await,
            "disable" => self.toggle_job(&input, false).await,
            "test" => self.test_job(&input).await,
            unknown => Ok(ToolResult::error(format!(
                "Unknown action '{unknown}'. Valid: create, list, delete, update, enable, disable, test"
            ))),
        }
    }
}

impl CronManageTool {
    async fn create_job(&self, input: &Value) -> Result<ToolResult> {
        let name = match input.get("name").and_then(|v| v.as_str()) {
            Some(n) if !n.is_empty() => n,
            _ => {
                return Ok(ToolResult::error(
                    "'name' is required for create".to_string(),
                ));
            }
        };

        let cron_expr = match input.get("cron").and_then(|v| v.as_str()) {
            Some(c) if !c.is_empty() => c,
            _ => {
                return Ok(ToolResult::error(
                    "'cron' expression is required for create".to_string(),
                ));
            }
        };

        // Validate cron expression (user provides 5-field, we prepend "0" for seconds)
        let cron_with_secs = format!("0 {cron_expr}");
        if let Err(e) = cron_with_secs.parse::<cron::Schedule>() {
            return Ok(ToolResult::error(format!(
                "Invalid cron expression '{cron_expr}': {e}. Use 5-field format: 'min hour dom mon dow'. \
                 Day-of-week is 1-7 = Sun-Sat (1=Sunday, 7=Saturday; 0 is invalid) — prefer names like \
                 Mon-Fri or Sun to avoid mistakes. Example: '0 9 * * *' = daily 9am, '0 9 * * Mon-Fri' = weekdays 9am."
            )));
        }

        let prompt = match input.get("prompt").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => p,
            _ => {
                return Ok(ToolResult::error(
                    "'prompt' is required for create".to_string(),
                ));
            }
        };

        // Check for duplicate name
        if let Ok(Some(_)) = self.repo.find_by_name(name).await {
            return Ok(ToolResult::error(format!(
                "A cron job named '{name}' already exists. Use a different name or delete the existing one first."
            )));
        }

        let tz = input
            .get("tz")
            .and_then(|v| v.as_str())
            .unwrap_or("UTC")
            .to_string();
        // Validate the timezone now — it's honored by the scheduler (jobs run
        // in this zone's wall clock, DST-aware), so an unknown zone must be
        // rejected here rather than silently falling back to UTC.
        let parsed_tz = match crate::cron::parse_timezone(&tz) {
            Some(t) => t,
            None => {
                return Ok(ToolResult::error(format!(
                    "Unknown timezone '{tz}'. Use an IANA name like 'America/New_York', 'Europe/London', 'Asia/Tokyo', or 'UTC'."
                )));
            }
        };
        let provider = input
            .get("provider")
            .and_then(|v| v.as_str())
            .map(String::from);
        let model = input
            .get("model")
            .and_then(|v| v.as_str())
            .map(String::from);
        let thinking = input
            .get("thinking")
            .and_then(|v| v.as_str())
            .unwrap_or("off")
            .to_string();
        let auto_approve = input
            .get("auto_approve")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let deliver_to = input
            .get("deliver_to")
            .and_then(|v| v.as_str())
            .map(String::from);
        let deliver_api_key = input
            .get("deliver_api_key")
            .and_then(|v| v.as_str())
            .map(String::from);

        let job = CronJob::new(
            name.to_string(),
            cron_expr.to_string(),
            tz,
            prompt.to_string(),
            provider,
            model,
            thinking,
            auto_approve,
            deliver_to.clone(),
            deliver_api_key,
        );

        let job_id = job.id.to_string();

        self.repo
            .insert(&job)
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        let delivery = deliver_to
            .as_deref()
            .unwrap_or("none (results logged only)");

        // Confirmation feedback: show the next few fire times in the job's
        // timezone so the agent (and user) can verify the schedule means what
        // they intended before treating it as done — this is what catches a
        // day-of-week mistake that still parses fine.
        let next_runs = crate::cron::format_upcoming(cron_expr, parsed_tz, 3, chrono::Utc::now());

        Ok(ToolResult::success(format!(
            "Cron job created:\n  ID: {job_id}\n  Name: {name}\n  Schedule: {cron_expr}\n  Timezone: {}\n  Deliver to: {delivery}\n  Enabled: true\n  Next runs:\n{next_runs}\n\nVerify the Next runs match what you intended (day-of-week is Sun-Sat, DST handled) before confirming to the user.",
            job.timezone
        )))
    }

    async fn update_job(&self, input: &Value) -> Result<ToolResult> {
        let job_id = match input.get("job_id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => id,
            _ => {
                return Ok(ToolResult::error(
                    "'job_id' is required for update".to_string(),
                ));
            }
        };

        // Resolve by ID first, then by name (same resolution as delete/test).
        let job = match self.repo.find_by_id(job_id).await {
            Ok(Some(j)) => j,
            Ok(None) => match self.repo.find_by_name(job_id).await {
                Ok(Some(j)) => j,
                _ => {
                    return Ok(ToolResult::error(format!(
                        "No cron job found with ID or name '{job_id}'."
                    )));
                }
            },
            Err(e) => {
                return Ok(ToolResult::error(format!("Error looking up cron job: {e}")));
            }
        };

        let mut patch = CronJobPatch::default();
        let mut changed: Vec<String> = Vec::new();
        let mut provided = 0usize;

        // Name (a rename checks for a collision with another job).
        if let Some(name) = input.get("name").and_then(|v| v.as_str()) {
            provided += 1;
            if name.is_empty() {
                return Ok(ToolResult::error("'name' cannot be empty".to_string()));
            }
            if name != job.name {
                let taken = matches!(
                    self.repo.find_by_name(name).await,
                    Ok(Some(other)) if other.id != job.id
                );
                if taken {
                    return Ok(ToolResult::error(format!(
                        "A cron job named '{name}' already exists. Use a different name."
                    )));
                }
                patch.name = Some(name.to_string());
                changed.push(format!("name -> {name}"));
            }
        }

        // Cron schedule: validated exactly like create.
        let mut schedule_changed = false;
        if let Some(cron_expr) = input.get("cron").and_then(|v| v.as_str()) {
            provided += 1;
            if cron_expr.is_empty() {
                return Ok(ToolResult::error("'cron' cannot be empty".to_string()));
            }
            let cron_with_secs = format!("0 {cron_expr}");
            if let Err(e) = cron_with_secs.parse::<cron::Schedule>() {
                return Ok(ToolResult::error(format!(
                    "Invalid cron expression '{cron_expr}': {e}. Use 5-field format: 'min hour dom mon dow'. \
                     Day-of-week is 1-7 = Sun-Sat (1=Sunday, 7=Saturday; 0 is invalid) — prefer names like \
                     Mon-Fri or Sun to avoid mistakes. Example: '0 9 * * *' = daily 9am, '0 9 * * Mon-Fri' = weekdays 9am."
                )));
            }
            if cron_expr != job.cron_expr {
                patch.cron_expr = Some(cron_expr.to_string());
                schedule_changed = true;
                changed.push(format!("schedule -> {cron_expr}"));
            }
        }

        // Timezone: validated exactly like create.
        let mut tz_changed = false;
        let effective_tz = if let Some(tz) = input.get("tz").and_then(|v| v.as_str()) {
            provided += 1;
            if tz.is_empty() {
                return Ok(ToolResult::error("'tz' cannot be empty".to_string()));
            }
            if crate::cron::parse_timezone(tz).is_none() {
                return Ok(ToolResult::error(format!(
                    "Unknown timezone '{tz}'. Use an IANA name like 'America/New_York', 'Europe/London', 'Asia/Tokyo', or 'UTC'."
                )));
            }
            if tz != job.timezone {
                patch.timezone = Some(tz.to_string());
                tz_changed = true;
                changed.push(format!("timezone -> {tz}"));
            }
            tz.to_string()
        } else {
            job.timezone.clone()
        };

        // Prompt: omitting it keeps the existing one (the whole point of a
        // patch-style update).
        if let Some(prompt) = input.get("prompt").and_then(|v| v.as_str()) {
            provided += 1;
            if prompt.is_empty() {
                return Ok(ToolResult::error("'prompt' cannot be empty".to_string()));
            }
            if prompt != job.prompt {
                patch.prompt = Some(prompt.to_string());
                changed.push(format!("prompt -> {}", truncate(prompt, 60)));
            }
        }

        // Optional string overrides: present = set, empty string = clear.
        for (key, current) in [
            ("provider", job.provider.as_deref()),
            ("model", job.model.as_deref()),
            ("deliver_to", job.deliver_to.as_deref()),
            ("deliver_api_key", job.deliver_api_key.as_deref()),
        ] {
            let (was_provided, value, line) = override_change(input, key, current);
            if !was_provided {
                continue;
            }
            provided += 1;
            let value = value.expect("provided override_change always returns Some");
            if !line.is_empty() {
                changed.push(line);
            }
            match key {
                "provider" => patch.provider = Some(value),
                "model" => patch.model = Some(value),
                "deliver_to" => patch.deliver_to = Some(value),
                _ => patch.deliver_api_key = Some(value),
            }
        }

        if let Some(thinking) = input.get("thinking").and_then(|v| v.as_str()) {
            provided += 1;
            if !matches!(thinking, "off" | "on" | "budget") {
                return Ok(ToolResult::error(format!(
                    "Invalid thinking mode '{thinking}'. Valid: off, on, budget."
                )));
            }
            if thinking != job.thinking {
                patch.thinking = Some(thinking.to_string());
                changed.push(format!("thinking -> {thinking}"));
            }
        }

        if let Some(auto) = input.get("auto_approve").and_then(|v| v.as_bool()) {
            provided += 1;
            if auto != job.auto_approve {
                patch.auto_approve = Some(auto);
                changed.push(format!("auto_approve -> {auto}"));
            }
        }

        if let Some(enabled) = input.get("enabled").and_then(|v| v.as_bool()) {
            provided += 1;
            if enabled != job.enabled {
                patch.enabled = Some(enabled);
                changed.push(format!("enabled -> {enabled}"));
            }
        }

        if provided == 0 {
            return Ok(ToolResult::error(
                "Nothing to update: provide at least one field to change (name, cron, tz, prompt, \
                 provider, model, thinking, auto_approve, deliver_to, deliver_api_key, enabled)."
                    .to_string(),
            ));
        }

        if changed.is_empty() {
            return Ok(ToolResult::success(format!(
                "Cron job '{}' (id={}) unchanged: every provided value already matches.",
                job.name, job.id
            )));
        }

        // A changed schedule or timezone must recompute the next fire time:
        // NULL it and let the scheduler recalculate on the next tick.
        patch.reset_next_run = schedule_changed || tz_changed;

        let updated = self
            .repo
            .update_fields(&job.id.to_string(), patch)
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        if !updated {
            return Ok(ToolResult::error(format!(
                "Failed to update cron job '{}' (id={}).",
                job.name, job.id
            )));
        }

        let mut out = format!("Cron job '{}' (id={}) updated:\n", job.name, job.id);
        for line in &changed {
            out.push_str(&format!("  {line}\n"));
        }

        if schedule_changed || tz_changed {
            let effective_cron = input
                .get("cron")
                .and_then(|v| v.as_str())
                .unwrap_or(&job.cron_expr);
            // Both were validated above, so this cannot fail.
            if let Some(tz) = crate::cron::parse_timezone(&effective_tz) {
                let next_runs =
                    crate::cron::format_upcoming(effective_cron, tz, 3, chrono::Utc::now());
                out.push_str(&format!(
                    "  Next runs (recomputed):\n{next_runs}\n\nVerify the Next runs match what you intended (day-of-week is Sun-Sat, DST handled) before confirming to the user."
                ));
            }
        }

        Ok(ToolResult::success(out))
    }

    async fn list_jobs(&self) -> Result<ToolResult> {
        let jobs = self
            .repo
            .list_all()
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        if jobs.is_empty() {
            return Ok(ToolResult::success("No cron jobs configured.".to_string()));
        }

        let lines: Vec<String> = jobs
            .iter()
            .map(|j| {
                let status = if j.enabled { "enabled" } else { "disabled" };
                let deliver = j.deliver_to.as_deref().unwrap_or("none");
                let last = j
                    .last_run_at
                    .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_else(|| "never".to_string());
                format!(
                    "- [{}] {} (id={})\n    Schedule: {} ({})\n    Deliver: {}\n    Last run: {}\n    Prompt: {}",
                    status,
                    j.name,
                    j.id,
                    j.cron_expr,
                    j.timezone,
                    deliver,
                    last,
                    truncate(&j.prompt, 80),
                )
            })
            .collect();

        Ok(ToolResult::success(format!(
            "Cron jobs ({}):\n{}",
            jobs.len(),
            lines.join("\n")
        )))
    }

    async fn delete_job(&self, input: &Value) -> Result<ToolResult> {
        let job_id = match input.get("job_id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => id,
            _ => {
                return Ok(ToolResult::error(
                    "'job_id' is required for delete".to_string(),
                ));
            }
        };

        // Step 1: Look up the job to show what will be deleted
        let job = match self.repo.find_by_id(job_id).await {
            Ok(Some(j)) => j,
            Ok(None) => {
                // Try by name
                match self.repo.find_by_name(job_id).await {
                    Ok(Some(j)) => j,
                    _ => {
                        return Ok(ToolResult::error(format!(
                            "No cron job found with ID or name '{job_id}'."
                        )));
                    }
                }
            }
            Err(e) => {
                return Ok(ToolResult::error(format!("Error looking up cron job: {e}")));
            }
        };

        let confirm = input
            .get("confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !confirm {
            // Safety check: show job details, do NOT delete
            let deliver = job.deliver_to.as_deref().unwrap_or("none");
            return Ok(ToolResult::success(format!(
                "⚠️  DELETE REQUEST (not yet executed)\n\n\
                 Job: {} (id={})\n\
                 Schedule: {} ({})\n\
                 Deliver: {}\n\
                 Prompt: {}\n\n\
                 To confirm deletion, call again with confirm=true.\n\
                 Use 'disable' to temporarily pause without losing the job.",
                job.name,
                job.id,
                job.cron_expr,
                job.timezone,
                deliver,
                truncate(&job.prompt, 120)
            )));
        }

        // Step 2: Back up all jobs before deleting
        if let Ok(all_jobs) = self.repo.list_all().await {
            self.backup_jobs(&all_jobs).await;
        }

        // Step 3: Actually delete
        let deleted = self
            .repo
            .delete(&job.id.to_string())
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        if deleted {
            Ok(ToolResult::success(format!(
                "Cron job '{}' (id={}) deleted. Backup saved to ~/.opencrabs/backups/cron/",
                job.name, job.id
            )))
        } else {
            Ok(ToolResult::error(format!(
                "Failed to delete cron job '{}' (id={}).",
                job.name, job.id
            )))
        }
    }

    async fn backup_jobs(&self, jobs: &[CronJob]) {
        use std::fs;
        use std::path::PathBuf;

        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let backup_dir = home.join(".opencrabs/backups/cron");
        if fs::create_dir_all(&backup_dir).is_err() {
            return;
        }

        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let backup_path = backup_dir.join(format!("cron-jobs-{timestamp}.json"));

        if let Ok(json) = serde_json::to_string_pretty(jobs) {
            let _ = fs::write(&backup_path, json);
        }

        // Rotate: keep only the 10 most recent backups
        if let Ok(entries) = fs::read_dir(&backup_dir) {
            let mut files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .map(|n| n.starts_with("cron-jobs-") && n.ends_with(".json"))
                        .unwrap_or(false)
                })
                .collect();
            files.sort_by_key(|e| e.file_name());
            if files.len() > 10 {
                for old in &files[..files.len() - 10] {
                    let _ = fs::remove_file(old.path());
                }
            }
        }
    }

    async fn toggle_job(&self, input: &Value, enabled: bool) -> Result<ToolResult> {
        let job_id = match input.get("job_id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => id,
            _ => {
                return Ok(ToolResult::error(
                    "'job_id' is required for enable/disable".to_string(),
                ));
            }
        };

        let updated = self
            .repo
            .set_enabled(job_id, enabled)
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        if updated {
            let state = if enabled { "enabled" } else { "disabled" };
            Ok(ToolResult::success(format!("Cron job {job_id} {state}.")))
        } else {
            Ok(ToolResult::error(format!(
                "No cron job found with ID '{job_id}'."
            )))
        }
    }

    async fn test_job(&self, input: &Value) -> Result<ToolResult> {
        let job_id = match input.get("job_id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => id,
            _ => {
                // Also accept name
                match input.get("name").and_then(|v| v.as_str()) {
                    Some(name) if !name.is_empty() => {
                        if let Ok(Some(job)) = self.repo.find_by_name(name).await {
                            return self.trigger_by_id(&job.id.to_string(), &job.name).await;
                        }
                        return Ok(ToolResult::error(format!(
                            "No cron job found with name '{name}'."
                        )));
                    }
                    _ => {
                        return Ok(ToolResult::error(
                            "'job_id' or 'name' is required for test".to_string(),
                        ));
                    }
                }
            }
        };

        // Try ID first, then name
        if let Ok(Some(job)) = self.repo.find_by_id(job_id).await {
            return self.trigger_by_id(&job.id.to_string(), &job.name).await;
        }
        if let Ok(Some(job)) = self.repo.find_by_name(job_id).await {
            return self.trigger_by_id(&job.id.to_string(), &job.name).await;
        }

        Ok(ToolResult::error(format!(
            "No cron job found with ID or name '{job_id}'."
        )))
    }

    async fn trigger_by_id(&self, id: &str, name: &str) -> Result<ToolResult> {
        let triggered = self
            .repo
            .trigger_now(id)
            .await
            .map_err(|e| super::error::ToolError::Execution(e.to_string()))?;

        if triggered {
            Ok(ToolResult::success(format!(
                "Cron job '{name}' (id={id}) triggered. It will execute on the next scheduler tick (within 60 seconds). Check logs for execution status."
            )))
        } else {
            Ok(ToolResult::error(format!(
                "Failed to trigger cron job '{id}'."
            )))
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max).collect::<String>())
    }
}

/// Helper for the optional string overrides on update (provider, model,
/// deliver_to, deliver_api_key). Returns whether the field was provided, the
/// tri-state value to store in the patch (`Some(None)` = clear back to NULL,
/// `Some(Some(v))` = set), and a human-readable change line that stays empty
/// when the value is unchanged.
fn override_change(
    input: &Value,
    key: &str,
    current: Option<&str>,
) -> (bool, Option<Option<String>>, String) {
    let Some(raw) = input.get(key).and_then(|v| v.as_str()) else {
        return (false, None, String::new());
    };
    let new_val: Option<String> = if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    };
    let differs = match (&new_val, current) {
        (Some(n), Some(c)) => n != c,
        (None, None) => false,
        _ => true,
    };
    let line = if differs {
        match &new_val {
            Some(n) => format!("{key} -> {n}"),
            None => format!("{key} -> (cleared)"),
        }
    } else {
        String::new()
    };
    (true, Some(new_val), line)
}
