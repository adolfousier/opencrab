//! Plan Management Tool
//!
//! Allows the LLM to create, update, and manage structured plans for complex tasks.

use super::error::{Result, ToolError};
use super::r#trait::{Tool, ToolCapability, ToolExecutionContext, ToolResult};
use crate::tui::plan::{PlanDocument, PlanStatus, PlanTask, TaskDep, TaskStatus, TaskType};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Plan management tool
pub struct PlanTool;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum PlanOperation {
    /// Create a new plan (design or checklist track) OR import one from a
    /// JSON file. Allowed from NoPlan or pre-init Editing only; a live
    /// post-init or Active plan must be discarded first.
    Init {
        /// Plan title (create mode). One of `title` / `file_path` is required.
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        context: String,
        #[serde(default)]
        risks: Vec<String>,
        #[serde(default)]
        test_strategy: String,
        #[serde(default)]
        technical_stack: Vec<String>,
        /// Import mode: absolute path to a plan JSON file on disk. Takes
        /// precedence over `title` and `mode` when present.
        #[serde(default)]
        file_path: Option<String>,
        /// Track selector: "design" (session .md + user Approve) or
        /// "checklist" (inline tasks, Active immediately). When omitted:
        /// tasks present imply checklist, no tasks imply design.
        #[serde(default)]
        mode: Option<String>,
        /// Inline task definitions (checklist mode): plan + tasks in one call.
        #[serde(default)]
        tasks: Vec<InlineTask>,
    },
    /// Append one or more tasks in a single call (primary append op).
    /// Active only: checklist operations are blocked while Editing.
    AddTasks { tasks: Vec<InlineTask> },
    /// Append a single task. Backward-compatible alias that behaves like
    /// `add_tasks` with one task.
    AddTask {
        title: String,
        #[serde(default)]
        description: String,
        #[serde(default = "default_task_type")]
        task_type: String,
        #[serde(default)]
        dependencies: Vec<usize>, // Task order numbers
        #[serde(default = "default_complexity")]
        complexity: u8,
        #[serde(default)]
        acceptance_criteria: Vec<String>,
    },
    /// Find and start the next task, or a specific one via `task_order`.
    /// Active only. Returns full task details. Idempotent on an in-progress
    /// task (re-surfaces its details after a compaction); resets a failed
    /// task for retry.
    Start {
        #[serde(default)]
        task_order: Option<usize>,
    },
    /// Finish a task and auto-start the next one (returning its full details).
    Complete {
        task_order: usize,
        /// "success" (default), "fail", or "skip".
        #[serde(default = "default_action")]
        action: String,
        #[serde(default)]
        output: String,
    },
    /// Self-approve the plan (Editing -> Active) WITHOUT the user's Approve
    /// button / `/execute`. Allowed ONLY when the user has granted autonomy for
    /// this session (`grant_autonomy`); otherwise refused so the default stays
    /// user-gated (#581).
    Approve,
    /// Grant this session autonomy so the agent may `approve` its own plans.
    /// Call this ONLY when the user explicitly says to proceed without waiting
    /// for approval ("go for it", "no hand-holding", "don't wait for me").
    /// Durable until revoked (#581).
    GrantAutonomy,
    /// Revoke this session's self-approval autonomy — future plans require the
    /// user's Approve again. Call when the user asks to go back to approving
    /// plans themselves (#581).
    RevokeAutonomy,
    /// Discard the live plan (remove its files → NoPlan). This is a USER
    /// action: the agent's call is refused unless the session has granted
    /// plan autonomy, because a model that can shred its own plan can
    /// wiggle out of the review harness mid-gate — or be told to by a
    /// malicious message. The user discards via /discard or the plan
    /// card's Discard button; both bypass this tool.
    Discard,
    /// Return the current plan state (title, status, checklist progress) with no
    /// side effects. Use to answer "what's the plan / where are we"; unlike
    /// `start` it never mutates the plan (#585).
    ShowPlan,
}

/// Inline task definition accepted by `init` so a plan and its tasks can be
/// created in a single call.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct InlineTask {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_task_type")]
    pub task_type: String,
    #[serde(default)]
    pub dependencies: Vec<usize>,
    #[serde(default = "default_complexity")]
    pub complexity: u8,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
}

pub(crate) fn default_complexity() -> u8 {
    3
}

fn default_task_type() -> String {
    "other".to_string()
}

fn default_action() -> String {
    "success".to_string()
}

/// Parse a task-type string into a `TaskType`, mapping anything unrecognized
/// to `Other` (lossless: the original string is preserved). The fallback is
/// logged so out-of-enum values stay observable.
fn parse_task_type(s: &str) -> TaskType {
    match s.to_lowercase().as_str() {
        "research" => TaskType::Research,
        "edit" => TaskType::Edit,
        "create" => TaskType::Create,
        "delete" => TaskType::Delete,
        "test" => TaskType::Test,
        "refactor" => TaskType::Refactor,
        "documentation" => TaskType::Documentation,
        "configuration" => TaskType::Configuration,
        "build" => TaskType::Build,
        "other" => TaskType::Other("other".to_string()),
        unknown => {
            tracing::debug!("Unknown task_type '{unknown}' mapped to the 'other' category");
            TaskType::Other(unknown.to_string())
        }
    }
}

/// Append a task to `plan`, resolving 1-based dependency order numbers to the
/// referenced tasks' UUIDs. Returns the new task's order. Shared by `add_task`
/// and `init`'s inline tasks.
fn add_task_to_plan(
    plan: &mut PlanDocument,
    title: String,
    description: String,
    task_type: &str,
    dependencies: &[usize],
    complexity: u8,
    acceptance_criteria: Vec<String>,
) -> Result<usize> {
    validate_string(&title, MAX_TITLE_LENGTH, "Task title")?;
    // Title and description are both required on each task (ADR 0003
    // checklist contract), so an empty description is refused, not skipped.
    validate_string(&description, MAX_DESCRIPTION_LENGTH, "Task description")?;
    let order = plan.tasks.len() + 1;
    let mut task = PlanTask::new(order, title, description, parse_task_type(task_type));
    task.complexity = complexity.clamp(1, 5);
    task.acceptance_criteria = acceptance_criteria;
    for dep_order in dependencies {
        if *dep_order == 0 {
            return Err(ToolError::InvalidInput(
                "Task numbers start at 1, not 0".to_string(),
            ));
        }
        let dep_task = plan.tasks.get(*dep_order - 1).ok_or_else(|| {
            ToolError::InvalidInput(format!(
                "Invalid dependency: task {dep_order} does not exist"
            ))
        })?;
        task.dependencies.push(TaskDep::Id(dep_task.id));
    }
    plan.tasks.push(task);
    Ok(order)
}

/// Deterministic refusal for checklist operations (`add_tasks`, `add_task`,
/// `start`, `complete`) attempted while the plan is not Active. `None`
/// means the operation may proceed (NoPlan falls through to the usual
/// "No active plan" error).
fn checklist_blocked_reason(state: crate::utils::plan_files::PlanModeState) -> Option<String> {
    use crate::utils::plan_files::PlanModeState;
    match state {
        PlanModeState::NoPlan | PlanModeState::Active => None,
        PlanModeState::PreInitEditing => Some(
            "No plan yet: the session is in Plan mode (pre-init Editing). Call 'init' \
             first: mode=\"checklist\" with inline tasks to execute now, or \
             mode=\"design\" to draft the plan for user Approve."
                .to_string(),
        ),
        PlanModeState::PostInitEditing => Some(
            "Checklist operations are blocked while the plan is being designed \
             (Editing). Refine the session plan .md and wait for the user to approve \
             the plan; the checklist goes live on Approve."
                .to_string(),
        ),
    }
}

/// Map a plan task outcome verb to an epistemic confidence level.
/// Pure — unit-testable without store access.
fn task_outcome_confidence(verb: &str) -> super::epistemic::Confidence {
    match verb {
        "completed" => super::epistemic::Confidence::Verified,
        "failed" => super::epistemic::Confidence::Contradicted,
        _ => super::epistemic::Confidence::Uncertain, // skipped
    }
}

/// Stable belief key for a plan task (order + title hash) so a retry that
/// later succeeds supersedes the earlier failure on the same key rather than
/// duplicating it. Pure — unit-testable without store access.
fn task_outcome_key(task_order: usize, title: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    title.hash(&mut hasher);
    format!("plan:task:{}:{:016x}", task_order, hasher.finish())
}

/// Epistemic engine integration (#862): log a plan task's outcome as a
/// belief, letting the engine record fail→success transitions across retries.
fn log_task_outcome_belief(
    task_order: usize,
    title: &str,
    verb: &str,
    output: &Option<String>,
    confidence_override: Option<super::epistemic::Confidence>,
) {
    let key = task_outcome_key(task_order, title);
    // A criteria-aware downgrade (#870) logs a success as Uncertain instead of
    // Verified when nothing mechanically checked the declared criteria.
    let confidence = confidence_override.unwrap_or_else(|| task_outcome_confidence(verb));

    let mut value = verb.to_string();
    if let Some(o) = output {
        let snippet: String = o.trim().chars().take(120).collect();
        if !snippet.is_empty() {
            value.push_str(&format!(" — {snippet}"));
        }
    }

    let result = super::epistemic::add_belief(&key, &value, confidence, "plan_tool:complete");
    if let super::epistemic::ContradictionResult::Contradicted {
        old_value,
        new_value,
    } = result
    {
        tracing::info!(
            "plan_tool: task #{task_order} outcome superseded — was '{old_value}', now '{new_value}'"
        );
    }
}

/// Clear the session goal a plan task set (on complete or skip).
async fn clear_task_goal(context: &ToolExecutionContext, session_id: uuid::Uuid) {
    let Some(svc) = context.service_context.as_ref() else {
        return;
    };
    if let Err(e) = crate::brain::goal::GoalManager::new(svc.clone())
        .clear_goal(session_id)
        .await
    {
        tracing::warn!("Failed to clear plan-task goal: {e}");
    }
}

/// One-line-per-task list (order, title, type) for the `init` confirmation.
fn render_task_list(plan: &PlanDocument) -> String {
    plan.tasks
        .iter()
        .map(|t| format!("  {}. {} [{}]", t.order, t.title, t.task_type))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Full task details (type, complexity, description, acceptance criteria,
/// dependency state, status) for `start` and `complete`'s next-task preview.
/// This is the recovery payload that survives a context compaction.
fn render_task_details(plan: &PlanDocument, task: &PlanTask) -> String {
    let criteria = if task.acceptance_criteria.is_empty() {
        String::new()
    } else {
        let lines = task
            .acceptance_criteria
            .iter()
            .map(|c| format!("  • {c}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("\nAcceptance Criteria:\n{lines}")
    };
    let deps = if task.dependencies.is_empty() {
        String::new()
    } else {
        let parts = task
            .dependencies
            .iter()
            .filter_map(|d| d.as_uuid())
            .filter_map(|id| plan.get_task(&id))
            .map(|t| {
                let mark = if matches!(t.status, TaskStatus::Completed | TaskStatus::Skipped) {
                    "✓"
                } else {
                    "✗"
                };
                format!("Task {} {}", t.order, mark)
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("\nDependencies: {parts}")
    };
    format!(
        "Type: {} | Complexity: {}\nDescription: {}{}{}\nStatus: {:?}",
        task.task_type,
        task.complexity_stars(),
        task.description,
        criteria,
        deps,
        task.status
    )
}

/// Validate plan file path for security
/// Prevents symlink attacks and path traversal
pub(crate) fn validate_plan_file_path(path: &Path, base_dir: &Path) -> Result<()> {
    // Check if path is absolute and within the base directory
    if !path.starts_with(base_dir) {
        return Err(ToolError::InvalidInput(
            "Plan file must be within the session directory".to_string(),
        ));
    }

    // Check for symlinks (security risk)
    if path.exists() {
        let metadata = std::fs::symlink_metadata(path).map_err(ToolError::Io)?;
        if metadata.is_symlink() {
            return Err(ToolError::InvalidInput(
                "Plan file cannot be a symlink (security restriction)".to_string(),
            ));
        }
    }

    // Verify filename matches pattern .opencrabs_plan_{uuid}.json (no traversal)
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ToolError::InvalidInput("Invalid plan filename".to_string()))?;

    if !file_name.starts_with(".opencrabs_plan_") || !file_name.ends_with(".json") {
        return Err(ToolError::InvalidInput(
            "Plan filename must match pattern .opencrabs_plan_{session_id}.json".to_string(),
        ));
    }

    // Extract and validate UUID portion
    let uuid_part = &file_name[16..file_name.len() - 5]; // Remove ".opencrabs_plan_" (16 chars) and ".json" (5 chars)
    uuid::Uuid::parse_str(uuid_part).map_err(|_| {
        ToolError::InvalidInput("Plan filename must contain a valid UUID".to_string())
    })?;

    Ok(())
}

/// Maximum plan file size (10MB)
pub(crate) const MAX_PLAN_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Input validation limits
pub(crate) const MAX_TITLE_LENGTH: usize = 200;
pub(crate) const MAX_DESCRIPTION_LENGTH: usize = 5000;
pub(crate) const MAX_CONTEXT_LENGTH: usize = 5000;

// ── Ralph Loop: Mechanical verification gate ────────────────────────
// Loaded from ~/.opencrabs/safety/ralph_loop.toml. The TOML defines
// verification commands per task type. Before accepting "success" on
// a task, the gate runs those commands and rejects the completion if
// any exit non-zero. This prevents the model from hallucinating
// "clippy passed" when it didn't.

use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
struct RalphLoopConfig {
    #[serde(default)]
    forward: RalphForward,
    #[serde(default)]
    verification: RalphVerification,
}

#[derive(Debug, Deserialize, Default)]
struct RalphForward {
    #[serde(default = "default_max_iterations")]
    max_iterations: u32,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct RalphVerification {
    #[serde(default)]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) require_all_pass: bool,
    #[serde(default)]
    pub(crate) task_type_commands: Vec<TaskTypeCommands>,
    /// What to do when a task declares acceptance criteria but its type has no
    /// verification commands (#870). `downgrade` (default) logs the completion
    /// as Uncertain instead of Verified; `strict` rejects it outright; `off`
    /// keeps the pre-#870 behaviour.
    #[serde(default)]
    pub(crate) criteria_policy: CriteriaPolicy,
}

/// Policy for completion claims made against unverified acceptance criteria (#870).
#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CriteriaPolicy {
    /// Accept the completion but log the belief as Uncertain, not Verified.
    #[default]
    Downgrade,
    /// Reject the completion: criteria were declared but nothing verifies them.
    Strict,
    /// Pre-#870 behaviour: criteria stay advisory, no enforcement.
    Off,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TaskTypeCommands {
    pub(crate) task_type: String,
    pub(crate) commands: Vec<String>,
}

fn default_max_iterations() -> u32 {
    20
}

/// The Ralph loop config, reloaded when the file changes on disk.
///
/// Was a `OnceLock`, so changing `max_iterations` or a verification command
/// needed a restart. Keyed on mtime + length now, so an edit takes effect on
/// the next task transition (#852).
fn ralph_loop_config() -> Option<std::sync::Arc<RalphLoopConfig>> {
    static CONFIG: OnceLock<Option<super::toml_hot_reload::HotToml<RalphLoopConfig>>> =
        OnceLock::new();
    CONFIG
        .get_or_init(|| {
            super::toml_hot_reload::safety_path("ralph_loop.toml")
                .map(|p| super::toml_hot_reload::HotToml::new(p, "Ralph loop config"))
        })
        .as_ref()?
        .get()
}

/// Run a shell command and return (exit_code, stdout+stderr).
fn run_verification_command(cmd: &str) -> (i32, String) {
    let output = std::process::Command::new("sh").arg("-c").arg(cmd).output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = format!("{stdout}{stderr}");
            (out.status.code().unwrap_or(-1), combined)
        }
        Err(e) => (-1, format!("Failed to execute verification command: {e}")),
    }
}

/// Truncate on a character boundary.
///
/// Slicing `&output[..500]` panics when byte 500 lands inside a multi-byte
/// character, which compiler output makes likely: rustc emits `^`, `─` and
/// smart quotes freely. A verification gate that panics on the failure it was
/// meant to report is worse than no gate.
pub(crate) fn truncate_output(output: &str, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        return output.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !output.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[truncated]", &output[..end])
}

/// Which commands verify a task of this type, if verification is on.
///
/// Pure over the config section so the mapping can be tested without a file on
/// disk. A type with no entry, or an entry with no commands, verifies nothing.
pub(crate) fn commands_for_type(
    verification: &RalphVerification,
    task_type: &str,
) -> Option<Vec<String>> {
    if !verification.enabled {
        return None;
    }
    let type_lower = task_type.to_lowercase();
    verification
        .task_type_commands
        .iter()
        .find(|tc| tc.task_type.to_lowercase() == type_lower)
        .filter(|tc| !tc.commands.is_empty())
        .map(|tc| tc.commands.clone())
}

/// The gate itself, over an injected runner.
///
/// `run` is a seam: production passes `run_verification_command`, tests pass a
/// fake so the verdict logic can be exercised without shelling out. Without it
/// nothing here was testable, because every path ran `sh -c`.
///
/// With `require_all` false the first failure returns immediately, so the
/// runner is NOT called for the remaining commands. That short-circuit is part
/// of the contract, not an optimisation: a cheap check placed first is meant to
/// spare an expensive one after it.
pub(crate) fn verify_with(
    task_type: &str,
    task_order: usize,
    commands: &[String],
    require_all: bool,
    run: &mut dyn FnMut(&str) -> (i32, String),
) -> std::result::Result<(), String> {
    let mut failures: Vec<String> = Vec::new();

    for cmd in commands {
        let (exit_code, output) = run(cmd);
        if exit_code == 0 {
            continue;
        }
        let truncated = truncate_output(&output, 500);
        failures.push(format!("`{cmd}` exited {exit_code}\n{truncated}"));
        tracing::warn!("Verification failed for task #{task_order}: {cmd} exited {exit_code}");
        if !require_all {
            return Err(format!(
                "Verification gate REJECTED for task #{task_order} (type={task_type}):\n{truncated}\n\n\
                 The command `{cmd}` exited with code {exit_code}. Fix the issue and try \
                 completing again. The Ralph loop does not accept self-reported success \
                 when verification commands fail."
            ));
        }
    }

    if failures.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Verification gate REJECTED for task #{task_order} (type={task_type}):\n{}\n\n\
         Fix the issues and try completing again. The Ralph loop does not accept \
         self-reported success when verification commands fail.",
        failures.join("\n\n")
    ))
}

/// What the verification gate decided about a completion claim (#870).
///
/// `verify_task_completion` used to return `Ok(())` for BOTH "commands ran and
/// passed" and "nothing was configured," so the caller couldn't tell a proven
/// completion from an unchecked one. The criteria policy needs that
/// distinction, so the gate now reports which happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerificationOutcome {
    /// Verification commands ran and all passed — the claim is proven.
    Verified,
    /// No commands configured for this task type — passed through unchecked.
    NotConfigured,
    /// Verification globally disabled (no config / `enabled = false`).
    Disabled,
}

/// Mechanical verification gate: run the verification commands for the task
/// type. `Ok(outcome)` reports how the claim was settled, `Err(message)` when a
/// configured command fails. This is the anti-hallucination gate — the model
/// cannot claim success when the shell says otherwise.
fn verify_task_completion(
    task_type: &str,
    task_order: usize,
) -> std::result::Result<VerificationOutcome, String> {
    let Some(config) = ralph_loop_config() else {
        return Ok(VerificationOutcome::Disabled);
    };
    if !config.verification.enabled {
        return Ok(VerificationOutcome::Disabled);
    }
    let Some(commands) = commands_for_type(&config.verification, task_type) else {
        return Ok(VerificationOutcome::NotConfigured);
    };

    tracing::info!(
        "Ralph loop verification for task #{task_order} (type={task_type}): {} commands",
        commands.len()
    );

    verify_with(
        task_type,
        task_order,
        &commands,
        config.verification.require_all_pass,
        &mut run_verification_command,
    )
    .map(|()| VerificationOutcome::Verified)
}

/// The verdict for a completion claim made against acceptance criteria (#870).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CriteriaVerdict {
    /// Accept normally; log the belief at the verb's usual confidence.
    Accept,
    /// Accept, but nothing mechanically verified the claim — log as Uncertain.
    Downgrade,
    /// Refuse the completion: criteria were declared but nothing verifies them.
    Reject,
}

/// Decide how to treat a success claim given the criteria policy (#870).
///
/// Pure so the whole policy matrix is unit-testable without a plan, a context,
/// or a config file on disk. The policy only bites when a task DECLARES
/// acceptance criteria yet its type ran NO verification commands
/// (`NotConfigured`). A proven completion (`Verified`), a globally disabled
/// gate (`Disabled` — an explicit user choice to turn the gate off), or a task
/// with no criteria always accepts.
pub(crate) fn criteria_verdict(
    policy: CriteriaPolicy,
    has_criteria: bool,
    outcome: VerificationOutcome,
) -> CriteriaVerdict {
    let unverified = matches!(outcome, VerificationOutcome::NotConfigured);
    if !(has_criteria && unverified) {
        return CriteriaVerdict::Accept;
    }
    match policy {
        CriteriaPolicy::Strict => CriteriaVerdict::Reject,
        CriteriaPolicy::Downgrade => CriteriaVerdict::Downgrade,
        CriteriaPolicy::Off => CriteriaVerdict::Accept,
    }
}

/// Validate string input
pub(crate) fn validate_string(s: &str, max_len: usize, field_name: &str) -> Result<()> {
    if s.is_empty() || s.trim().is_empty() {
        return Err(ToolError::InvalidInput(format!(
            "{} cannot be empty",
            field_name
        )));
    }

    if s.len() > max_len {
        return Err(ToolError::InvalidInput(format!(
            "{} exceeds maximum length of {} characters (got {})",
            field_name,
            max_len,
            s.len()
        )));
    }

    Ok(())
}

#[async_trait]
impl Tool for PlanTool {
    fn name(&self) -> &str {
        "plan"
    }

    fn description(&self) -> &str {
        "Manage a structured task plan for multi-step work. TWO TRACKS on `init`: \
         checklist (`mode`=\"checklist\", or inline `tasks` present) goes to Editing first \
         so the user can review before execution; design (`mode`=\"design\", or no tasks) \
         creates a session plan .md to refine and WAITS for the user to Approve it. Both \
         tracks go Active only on user Approve. While a plan is Editing, checklist \
         operations are refused and only the session .md is writable (design track). \
         \n\nOPERATIONS: `init` (create a plan, or import from a JSON `file_path`; allowed only \
         when no plan is live), `add_tasks` (append one or more tasks in a single call — the \
         primary append op), `add_task` (alias appending a single task), `start` (begin the \
         next task, or a specific one via `task_order`), `complete` (finish a task and \
         auto-start the next), `discard` (USER-ONLY: refused for the agent unless the \
         session granted plan autonomy — the user discards via /discard or the plan card's \
         Discard button), `show_plan` (read-only: report the current plan + checklist progress, e.g. \
         to answer 'what's the plan / where are we'). `add_tasks`/`add_task`/`start`/`complete` \
         are Active-only. \
         \n\nAPPROVAL: by default a plan waits in Editing for the USER to Approve (button / \
         /execute) before `start` works. If the user grants autonomy ('go for it', 'no \
         hand-holding', 'don't wait for me'), call `grant_autonomy` then `approve` to \
         self-approve and proceed WITHOUT the user; `approve` is refused unless autonomy was \
         granted. Even with autonomy you may still leave the plan for the user to Approve when \
         you judge it needs review. `revoke_autonomy` turns self-approval back off. \
         \n\nFLOW (checklist, no autonomy): init with `tasks` → WAIT for user Approve → start → (do the work) → complete → \
         (auto-starts next) → complete → … `start` and `complete` return the FULL task details \
         (description, acceptance criteria, dependencies), so the plan doubles as durable \
         memory across context compactions — call `start` with no args to re-surface the \
         in-progress task's details after a compaction. \
         \n\nWHEN TO USE: call `plan` BEFORE starting any task with 3+ distinct steps, dependencies \
         between steps, that touches multiple files, or that spans multiple commits; when the user \
         asks for a plan/roadmap; when a request describes >2 deliverables; or when the user will \
         step away while you work. Skip planning for trivial single-tool answers. \
         \n\nMARK PROGRESS AS YOU GO: after each task's work is VERIFIED done (command exited 0, file \
         written, tests/clippy pass), immediately call `complete` for it before moving on. The TUI \
         progress widget counts only completed tasks, so a stale 0/N while work is done means a \
         `complete` was skipped, not that progress is tracked some other way. \
         \n\nDETAILS: `start` is idempotent on an in-progress task and resets a failed task for \
         retry. `complete` takes action=\"success\" (default), \"fail\", or \"skip\". Ordering of \
         dependencies is by task order number (1-based). A started task's acceptance criteria \
         become the session goal until it completes. Completing the last task archives the plan. \
         \n\nIMPORT: `init` with an absolute `file_path` (non-empty tasks required) goes Active. \
         BUNDLED REFERENCE PLANS: source at `src/docs/reference/plans/` (embedded), runtime at \
         `~/.opencrabs/profiles/<profile>/plans/`. See `coding-plans/rust-fast.json` etc. and \
         `plan-json-spec.md`. \
         \n\nRE-TESTING AFTER BUG FIX: plans are forward-only — a completed task stays completed. \
         If a later task introduces a bug an earlier test would catch, `add_tasks` a new test task \
         rather than re-opening the completed one."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["init", "add_tasks", "add_task", "start", "complete", "approve", "discard", "show_plan", "grant_autonomy", "revoke_autonomy"],
                    "description": "init (create/import a plan), add_tasks (append one or more tasks — primary), add_task (alias, single task), start (begin next/specific task), complete (finish a task, auto-start next), approve (self-approve Editing→Active, only if the user granted autonomy), discard (abandon the live plan → no plan; call only when the user asks to scrap/replace it), show_plan (return the current plan state, read-only), grant_autonomy (allow self-approval this session — call only when the user says to proceed without approving), revoke_autonomy (require user Approve again)"
                },
                "mode": {
                    "type": "string",
                    "enum": ["design", "checklist"],
                    "description": "init track: design (session .md, wait for user Approve; requires empty tasks) or checklist (inline tasks, Active immediately). Omitted: tasks present imply checklist, none imply design."
                },
                "title": {
                    "type": "string",
                    "description": "Plan title (init create mode) or task title (add_task)"
                },
                "description": {
                    "type": "string",
                    "description": "Task description (add_task)"
                },
                "context": {
                    "type": "string",
                    "description": "Context and assumptions (init create mode)"
                },
                "risks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Identified risks and unknowns (init create mode)"
                },
                "test_strategy": {
                    "type": "string",
                    "description": "Testing strategy and approach (init create mode)"
                },
                "technical_stack": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Technologies, frameworks, and tools used (init create mode)"
                },
                "file_path": {
                    "type": "string",
                    "description": "Import mode: absolute path to a plan JSON file on disk (init). Takes precedence over title."
                },
                "tasks": {
                    "type": "array",
                    "items": { "type": "object" },
                    "description": "Task definitions for init checklist mode or add_tasks — each: {title, description?, task_type?, complexity?, dependencies?, acceptance_criteria?}. init with tasks creates the plan and all tasks in one call; add_tasks appends (at least one)."
                },
                "task_type": {
                    "type": "string",
                    "enum": ["research", "edit", "create", "delete", "test", "refactor", "documentation", "configuration", "build"],
                    "description": "Type of task (add_task; defaults to other)"
                },
                "dependencies": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "Task order numbers (1-based) that must complete first (add_task)"
                },
                "complexity": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 5,
                    "default": 3,
                    "description": "Task complexity from 1 (simple) to 5 (very complex) (add_task)"
                },
                "acceptance_criteria": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Acceptance criteria for task completion (add_task)"
                },
                "task_order": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Task number (1-based). Required for complete; optional for start (omit to pick the next task)."
                },
                "action": {
                    "type": "string",
                    "enum": ["success", "fail", "skip"],
                    "description": "How a task finished (complete): success (default), fail (retry later via start), or skip"
                },
                "output": {
                    "type": "string",
                    "description": "Task result / output, stored on the task (complete)"
                }
            },
            "required": ["operation"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::PlanManagement]
    }

    fn requires_approval(&self) -> bool {
        false
    }

    fn requires_approval_for_input(&self, input: &Value) -> bool {
        // `init` is the one user-visible gate: it establishes (creates or imports)
        // the plan the agent is about to execute, the same role the old
        // create/import/finalize approval served. `start`/`complete` then flow
        // without per-task prompts.
        input
            .get("operation")
            .and_then(|v| v.as_str())
            .map(|op| op == "init")
            .unwrap_or(false)
    }

    fn validate_input(&self, input: &Value) -> Result<()> {
        let _: PlanOperation = serde_json::from_value(input.clone())
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {}", e)))?;
        Ok(())
    }

    async fn execute(&self, input: Value, context: &ToolExecutionContext) -> Result<ToolResult> {
        let operation: PlanOperation = serde_json::from_value(input)?;

        // Load or create plan state from context (session-scoped)
        let session_dir = crate::config::opencrabs_home()
            .join("agents")
            .join("session");
        let _ = std::fs::create_dir_all(&session_dir);
        let plan_filename = format!(".opencrabs_plan_{}.json", context.session_id);
        let plan_file = session_dir.join(&plan_filename);

        // Security: Validate plan file path
        validate_plan_file_path(&plan_file, &session_dir)?;

        // Load through the shared plan store: legacy statuses map onto
        // Editing/Active, terminal legacy files resolve (Completed archives,
        // Cancelled deletes), old draft checklists normalize to Active, and
        // the size guard applies. The engine's lifecycle state (NoPlan /
        // pre-init / post-init Editing / Active) is derived from the same
        // files.
        let mut plan: Option<PlanDocument> =
            crate::utils::plan_files::load_plan(context.session_id).await;
        let state = crate::utils::plan_files::plan_mode_state(context.session_id).await;

        let result = match operation {
            PlanOperation::Init {
                title,
                context: ctx,
                risks,
                test_strategy,
                technical_stack,
                file_path,
                mode,
                tasks,
            } => {
                use crate::utils::plan_files::PlanModeState;
                // A live plan blocks re-init. Pre-init is NOT live for this
                // rule: the first successful init upgrades or replaces the
                // minimal sidecar, so users who typed /plan and changed
                // their mind are never trapped.
                match state {
                    PlanModeState::PostInitEditing => {
                        return Ok(ToolResult::error(
                            "A design plan is already live for this session (Editing). \
                             Refine the session plan .md and wait for user Approve, or \
                             ask the user to /discard it before creating a new plan."
                                .to_string(),
                        ));
                    }
                    PlanModeState::Active => {
                        return Ok(ToolResult::error(
                            "A checklist is already Active for this session. Complete \
                             its remaining tasks, or ask the user to /discard it before \
                             creating a new plan."
                                .to_string(),
                        ));
                    }
                    PlanModeState::NoPlan | PlanModeState::PreInitEditing => {}
                }

                if let Some(path) = file_path {
                    // ===== import mode (mode param ignored) =====
                    let import_path = std::path::Path::new(&path);
                    if !import_path.is_absolute() {
                        return Err(ToolError::InvalidInput(
                            "Import path must be absolute".to_string(),
                        ));
                    }
                    // Reject a symlink AT the target (don't walk ancestors — on
                    // macOS /var is a symlink and would reject every temp path).
                    if import_path.exists() {
                        let meta = std::fs::symlink_metadata(import_path).map_err(ToolError::Io)?;
                        if meta.is_symlink() {
                            return Err(ToolError::InvalidInput(
                                "Import path contains a symlink (security restriction)".to_string(),
                            ));
                        }
                    }
                    let metadata = tokio::fs::metadata(&import_path)
                        .await
                        .map_err(ToolError::Io)?;
                    if metadata.len() > MAX_PLAN_FILE_SIZE {
                        return Err(ToolError::InvalidInput(format!(
                            "Import file too large: {} bytes (max: {} bytes)",
                            metadata.len(),
                            MAX_PLAN_FILE_SIZE
                        )));
                    }
                    let content = tokio::fs::read_to_string(&import_path)
                        .await
                        .map_err(ToolError::Io)?;
                    let mut imported: PlanDocument = serde_json::from_str(&content)
                        .map_err(|e| ToolError::InvalidInput(format!("Invalid plan JSON: {e}")))?;

                    // The spec requires 3 root fields (title, description,
                    // tasks). PlanDocument defaults title/description so the
                    // minimal pre-init sidecar still parses, so the required
                    // contract is enforced here at the import boundary.
                    let mut missing_root = Vec::new();
                    if imported.title.trim().is_empty() {
                        missing_root.push("'title'");
                    }
                    if imported.description.trim().is_empty() {
                        missing_root.push("'description'");
                    }
                    if !missing_root.is_empty() {
                        return Ok(ToolResult::error(format!(
                            "Import refused: root {} must be present and non-empty \
                             (plan-json-spec.md requires title, description, and tasks)",
                            missing_root.join(" and ")
                        )));
                    }

                    // Same contract per task: title and description are both
                    // required (ADR 0003). Serde requires the fields to be
                    // present, so only blank values need catching here.
                    for (idx, task) in imported.tasks.iter().enumerate() {
                        if task.title.trim().is_empty() || task.description.trim().is_empty() {
                            return Ok(ToolResult::error(format!(
                                "Import refused: task {} must have a non-empty title and \
                                 description (plan-json-spec.md requires both per task)",
                                idx + 1
                            )));
                        }
                    }

                    if let Some(existing_plan) = plan.as_ref() {
                        tracing::info!(
                            "Importing plan '{}' over existing plan '{}'",
                            imported.title,
                            existing_plan.title
                        );
                    }

                    // Reassign fresh UUIDs and remap dependency references.
                    let old_to_new: std::collections::HashMap<uuid::Uuid, uuid::Uuid> = imported
                        .tasks
                        .iter()
                        .map(|t| (t.id, uuid::Uuid::new_v4()))
                        .collect();

                    imported.id = uuid::Uuid::new_v4();
                    imported.session_id = context.session_id;
                    // Import is checklist-track: tasks are already structured,
                    // but the plan still goes to Editing first so the user can
                    // review before execution starts (start/complete are blocked
                    // until approval).
                    imported.status = PlanStatus::Editing;
                    imported.created_at = Utc::now();
                    imported.updated_at = Utc::now();
                    imported.approved_at = None;

                    imported.resolve_index_deps();

                    for task in &imported.tasks {
                        for dep in &task.dependencies {
                            if let Some(dep_id) = dep.as_uuid()
                                && !old_to_new.contains_key(&dep_id)
                            {
                                return Err(ToolError::InvalidInput(format!(
                                    "Task '{}' depends on unknown task id {}",
                                    task.title, dep_id
                                )));
                            }
                        }
                    }

                    // Imported tasks start fresh.
                    for (idx, task) in imported.tasks.iter_mut().enumerate() {
                        let new_id = old_to_new[&task.id];
                        task.id = new_id;
                        // order is auto-assigned from array position (1-based):
                        // the schema marks it Do-NOT-Provide, and dependency
                        // resolution already ran above, so overwrite it here so
                        // an omitted order never lands a task at 0 (which would
                        // collide every task on get_task_by_order).
                        task.order = idx + 1;
                        // complexity defaults to 3 when omitted (deserializes to
                        // 0) and clamps to the 1-5 scale otherwise, matching the
                        // add_task path.
                        task.complexity = if task.complexity == 0 {
                            default_complexity()
                        } else {
                            task.complexity.clamp(1, 5)
                        };
                        task.status = TaskStatus::Pending;
                        task.completed_at = None;
                        task.retry_count = 0;
                        task.notes = None;
                        task.dependencies = task
                            .dependencies
                            .iter()
                            .filter_map(|dep| {
                                dep.as_uuid()
                                    .and_then(|old_id| old_to_new.get(&old_id).copied())
                                    .map(TaskDep::Id)
                            })
                            .collect();
                    }

                    imported.validate_dependencies().map_err(|e| {
                        ToolError::InvalidInput(format!(
                            "Imported plan has invalid dependencies: {e}"
                        ))
                    })?;

                    if imported.tasks.is_empty() {
                        return Ok(ToolResult::error(
                            "Empty import is refused: the plan file has no tasks. Import \
                             needs a structured checklist; to design a plan from scratch, \
                             call init with mode=\"design\" instead."
                                .to_string(),
                        ));
                    }

                    let count = imported.tasks.len();
                    let plan_title = imported.title.clone();
                    let list = render_task_list(&imported);
                    plan = Some(imported);

                    format!(
                        "📋 Imported plan: {plan_title} ({count} tasks)\n\n{list}\n\n\
                         Call 'start' to begin — it returns the first task's full details."
                    )
                } else {
                    // ===== create mode =====
                    let title = title.ok_or_else(|| {
                        ToolError::InvalidInput(
                            "init requires either 'title' (create) or 'file_path' (import)"
                                .to_string(),
                        )
                    })?;
                    validate_string(&title, MAX_TITLE_LENGTH, "Plan title")?;
                    if !ctx.is_empty() {
                        validate_string(&ctx, MAX_CONTEXT_LENGTH, "Plan context")?;
                    }

                    // Track disambiguation: explicit mode wins; otherwise
                    // tasks present imply checklist and no tasks imply design.
                    let design = match mode.as_deref() {
                        Some("design") => {
                            if !tasks.is_empty() {
                                return Ok(ToolResult::error(
                                    "mode=\"design\" with inline tasks is refused: a design \
                                     plan starts as prose in the session .md and gets its \
                                     checklist after user Approve. Either drop the tasks, or \
                                     use mode=\"checklist\" to go Active with them now."
                                        .to_string(),
                                ));
                            }
                            true
                        }
                        Some("checklist") => {
                            if tasks.is_empty() {
                                return Ok(ToolResult::error(
                                    "mode=\"checklist\" with no tasks is refused: a checklist \
                                     init needs at least one inline task. Provide `tasks`, or \
                                     use mode=\"design\" to draft the plan as prose first."
                                        .to_string(),
                                ));
                            }
                            false
                        }
                        Some(other) => {
                            return Ok(ToolResult::error(format!(
                                "Unknown mode '{other}'. Use \"design\" (session .md, user \
                                 Approve) or \"checklist\" (inline tasks, Active now)."
                            )));
                        }
                        None => tasks.is_empty(),
                    };

                    // Yolo design gate: only an explicit `/plan` slash arms
                    // the review pause (PreInitEditing with Slash origin).
                    // Anything else under auto-approve — agent-initiated
                    // design, or a keyword soft-nudge — keeps the rush
                    // behavior and is refused toward the checklist track.
                    let slash_armed = matches!(state, PlanModeState::PreInitEditing)
                        && matches!(
                            crate::utils::plan_files::pre_init_origin(context.session_id).await,
                            crate::utils::plan_files::PreInitOrigin::Slash
                        );
                    if design && context.auto_approve && !slash_armed {
                        return Ok(ToolResult::error(
                            "The design track under tool auto-approve is available only \
                             when the user entered Plan mode with the /plan command (the \
                             review gate). Use mode=\"checklist\" with inline tasks to \
                             proceed without a review pause, or ask the user to type \
                             /plan first."
                                .to_string(),
                        ));
                    }

                    if let Some(existing_plan) = plan.as_ref() {
                        tracing::info!(
                            "Replacing pre-init sidecar '{}' with new plan '{}'",
                            existing_plan.title,
                            title
                        );
                    }

                    let mut new_plan = PlanDocument::new(context.session_id, title.clone());
                    new_plan.context = ctx;
                    new_plan.risks = risks;
                    new_plan.test_strategy = test_strategy;
                    new_plan.technical_stack = technical_stack;
                    new_plan.status = PlanStatus::Editing;

                    for it in tasks {
                        add_task_to_plan(
                            &mut new_plan,
                            it.title,
                            it.description,
                            &it.task_type,
                            &it.dependencies,
                            it.complexity,
                            it.acceptance_criteria,
                        )?;
                    }

                    // Under tool auto-approve (yolo / cron / run / a2a) there is
                    // no user Approve step, so a checklist plan goes live now
                    // instead of stalling in Editing with nobody to approve it
                    // (#581 — the previous behavior left it stuck).
                    let auto_active = !design && context.auto_approve;
                    if auto_active {
                        new_plan.approve();
                    }

                    let count = new_plan.tasks.len();
                    plan = Some(new_plan);

                    let md_path =
                        crate::utils::plan_files::create_design_md(context.session_id, &title)
                            .await
                            .map_err(ToolError::Io)?;

                    if design {
                        format!(
                            "📋 Created design plan: {title} (Editing)\n\n\
                             Plan document: {}\n\n\
                             Write the design there (fill ## Context and the numbered \
                             ## Implementation steps), then WAIT for the user to approve \
                             the plan. Do NOT call 'start': checklist operations stay \
                             blocked until the plan is Active.",
                            md_path.display()
                        )
                    } else if auto_active {
                        format!(
                            "📋 Created plan: {title} ({count} tasks, Active — auto-approve). \
                             No user Approve step in this mode; call 'start' to begin executing."
                        )
                    } else {
                        format!(
                            "📋 Created plan: {title} ({count} tasks, Editing).\n\n\
                             The task list is ALREADY shown to the user in the plan card \
                             (the message carrying the Approve/Discard buttons). Do NOT \
                             repeat the tasks in your reply — that duplicates the card. \
                             Confirm the plan is ready in one short line and ask the user \
                             to approve.\n\n\
                             Plan document: {}\n\n\
                             WAIT for the user to approve the plan before calling 'start'. \
                             Checklist operations are blocked until approval.",
                            md_path.display()
                        )
                    }
                }
            }

            PlanOperation::AddTasks { tasks } => {
                if let Some(reason) = checklist_blocked_reason(state) {
                    return Ok(ToolResult::error(reason));
                }
                let current_plan = plan.as_mut().ok_or_else(|| {
                    ToolError::InvalidInput(
                        "No active plan. Create one with 'init' first.".to_string(),
                    )
                })?;
                if tasks.is_empty() {
                    return Ok(ToolResult::error(
                        "add_tasks needs at least one task in `tasks`.".to_string(),
                    ));
                }
                let mut added: Vec<String> = Vec::new();
                for it in tasks {
                    let task_title = it.title.clone();
                    let order = add_task_to_plan(
                        current_plan,
                        it.title,
                        it.description,
                        &it.task_type,
                        &it.dependencies,
                        it.complexity,
                        it.acceptance_criteria,
                    )?;
                    added.push(format!("  {order}. {task_title}"));
                }
                let total = current_plan.tasks.len();
                format!(
                    "✓ Added {} task(s):\n{}\n  Plan now has {total} tasks.",
                    added.len(),
                    added.join("\n")
                )
            }

            PlanOperation::AddTask {
                title,
                description,
                task_type,
                dependencies,
                complexity,
                acceptance_criteria,
            } => {
                if let Some(reason) = checklist_blocked_reason(state) {
                    return Ok(ToolResult::error(reason));
                }
                let current_plan = plan.as_mut().ok_or_else(|| {
                    ToolError::InvalidInput(
                        "No active plan. Create one with 'init' first.".to_string(),
                    )
                })?;
                let order = add_task_to_plan(
                    current_plan,
                    title.clone(),
                    description,
                    &task_type,
                    &dependencies,
                    complexity,
                    acceptance_criteria,
                )?;
                let total = current_plan.tasks.len();
                let ttype = current_plan
                    .get_task_by_order(order)
                    .unwrap()
                    .task_type
                    .clone();
                format!(
                    "✓ Added task #{order}: {title}\n  Type: {ttype} | Complexity: {}★\n  Position: {order} of {total}",
                    complexity.clamp(1, 5)
                )
            }

            PlanOperation::Start { task_order } => {
                if let Some(reason) = checklist_blocked_reason(state) {
                    return Ok(ToolResult::error(reason));
                }
                let current_plan = plan.as_mut().ok_or_else(|| {
                    ToolError::InvalidInput(
                        "No active plan. Create one with 'init' first.".to_string(),
                    )
                })?;
                if current_plan.tasks.is_empty() {
                    return Ok(ToolResult::error(
                        "Plan has no tasks yet. Add tasks with 'add_tasks' first.".to_string(),
                    ));
                }

                // Resolve which task to start.
                let target_order: Option<usize> = match task_order {
                    Some(o) => {
                        if current_plan.get_task_by_order(o).is_none() {
                            return Ok(ToolResult::error(format!("Task #{o} does not exist.")));
                        }
                        Some(o)
                    }
                    // No arg: resume an in-progress task first (compaction
                    // recovery), otherwise pick the next pending task.
                    None => current_plan
                        .tasks
                        .iter()
                        .find(|t| matches!(t.status, TaskStatus::InProgress))
                        .map(|t| t.order)
                        .or_else(|| current_plan.next_executable_task().map(|t| t.order)),
                };

                match target_order {
                    None => {
                        if current_plan.tasks.iter().all(|t| {
                            matches!(t.status, TaskStatus::Completed | TaskStatus::Skipped)
                        }) {
                            format!(
                                "✅ Plan complete. All {} tasks done. The plan is archived; \
                                 the session has no live plan.",
                                current_plan.tasks.len()
                            )
                        } else {
                            let blocked = current_plan
                                .tasks
                                .iter()
                                .filter(|t| matches!(t.status, TaskStatus::Pending))
                                .map(|t| format!("  ⊘ Task #{}: {}", t.order, t.title))
                                .collect::<Vec<_>>()
                                .join("\n");
                            format!(
                                "No task is ready to start — remaining tasks are blocked by \
                                 incomplete dependencies or failed tasks:\n{blocked}"
                            )
                        }
                    }
                    Some(order) => {
                        let status = current_plan
                            .get_task_by_order(order)
                            .unwrap()
                            .status
                            .clone();
                        // Starting a pending task requires its dependencies done.
                        if matches!(status, TaskStatus::Pending) {
                            let task = current_plan.get_task_by_order(order).unwrap();
                            if !current_plan.dependencies_satisfied(task) {
                                let unmet = task
                                    .dependencies
                                    .iter()
                                    .filter_map(|d| d.as_uuid())
                                    .filter_map(|id| current_plan.get_task(&id))
                                    .filter(|dep| {
                                        !matches!(
                                            dep.status,
                                            TaskStatus::Completed | TaskStatus::Skipped
                                        )
                                    })
                                    .map(|dep| format!("Task {}", dep.order))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                return Ok(ToolResult::error(format!(
                                    "⊘ Task #{order} blocked: waiting on {unmet}."
                                )));
                            }
                        }

                        let already_done =
                            matches!(status, TaskStatus::Completed | TaskStatus::Skipped);
                        if !already_done {
                            // Ralph loop: iteration cap. If the task has been
                            // retried too many times, block the start
                            // mechanically. The model cannot hallucinate its
                            // way past this gate.
                            if matches!(status, TaskStatus::Failed) {
                                let task = current_plan.get_task_by_order(order).unwrap();
                                let current_retries = task.retry_count;
                                let max_iter = ralph_loop_config()
                                    .map(|c| c.forward.max_iterations)
                                    .unwrap_or(20);
                                if (current_retries as u32) >= max_iter {
                                    return Ok(ToolResult::error(format!(
                                        "🔒 Ralph Loop iteration cap REACHED for task #{order}.\n\n\
                                         This task has failed {current_retries} times (max: {max_iter}). \
                                         The task is mechanically blocked. Either redesign the approach, \
                                         break it into smaller tasks, or ask the user to override."
                                    )));
                                }
                                tracing::info!(
                                    "Ralph loop: task #{order} retry {}/{max_iter}",
                                    current_retries + 1
                                );
                            }
                            // start() sets the task InProgress (also resets a
                            // Failed task for retry). Increments retry_count
                            // when coming from Failed state.
                            current_plan.get_task_by_order_mut(order).unwrap().start();
                            current_plan.status = PlanStatus::Active;
                        }

                        let done = current_plan
                            .tasks
                            .iter()
                            .filter(|t| matches!(t.status, TaskStatus::Completed))
                            .count();
                        let total = current_plan.tasks.len();
                        let task = current_plan.get_task_by_order(order).unwrap();
                        let details = render_task_details(current_plan, task);

                        // Epistemic Orient gate (#886): surface relevant
                        // low-confidence beliefs so the agent sees prior
                        // failures / uncertain outcomes before starting work.
                        let epistemic_note = {
                            let relevant: Vec<String> =
                                super::epistemic::list_by_prefix("plan:task:")
                                    .into_iter()
                                    .filter(|b| {
                                        matches!(
                                            b.confidence,
                                            super::epistemic::Confidence::Contradicted
                                                | super::epistemic::Confidence::Uncertain
                                        )
                                    })
                                    .map(|b| {
                                        format!(
                                            "  ⚠ [{}] {}: {}",
                                            b.confidence.label(),
                                            b.key,
                                            b.value
                                        )
                                    })
                                    .collect();
                            if relevant.is_empty() {
                                String::new()
                            } else {
                                format!("\n\nEpistemic flags ({}):", relevant.len())
                                    + "\n"
                                    + &relevant.join("\n")
                            }
                        };

                        if already_done {
                            format!(
                                "Task #{order}: {} — already {status:?}.\n\n{details}{epistemic_note}\n\n\
                                 Progress: {done}/{total} done.",
                                task.title
                            )
                        } else {
                            format!(
                                "▶️ Task #{order}: {}\n\n{details}{epistemic_note}\n\n\
                                 Progress: {done}/{total} done. Do the work, then call 'complete' \
                                 with task_order={order}.",
                                task.title
                            )
                        }
                    }
                }
            }

            PlanOperation::Complete {
                task_order,
                action,
                output,
            } => {
                if let Some(reason) = checklist_blocked_reason(state) {
                    return Ok(ToolResult::error(reason));
                }
                let current_plan = plan
                    .as_mut()
                    .ok_or_else(|| ToolError::InvalidInput("No active plan.".to_string()))?;
                if current_plan.get_task_by_order(task_order).is_none() {
                    return Ok(ToolResult::error(format!(
                        "Task #{task_order} does not exist."
                    )));
                }

                let out = if output.trim().is_empty() {
                    None
                } else {
                    Some(output.clone())
                };

                // ── Ralph Loop: mechanical verification gate ────────
                // Before accepting "success," run verification commands
                // from ralph_loop.toml. Non-zero exit = rejection.
                // The model cannot hallucinate "tests passed" when the
                // shell says otherwise.
                //
                // Criteria-aware (#870): a success claimed against stated
                // acceptance criteria that nothing mechanically verified is
                // downgraded to an Uncertain belief (default) or rejected
                // outright under `criteria_policy = "strict"`.
                let mut criteria_downgraded = false;
                if action.to_lowercase() == "success" {
                    let (task_type_str, has_criteria) = current_plan
                        .get_task_by_order(task_order)
                        .map(|t| (t.task_type.to_string(), !t.acceptance_criteria.is_empty()))
                        .unwrap_or_default();
                    match verify_task_completion(&task_type_str, task_order) {
                        Err(verify_msg) => {
                            return Ok(ToolResult::error(format!(
                                "🔒 Ralph Loop verification REJECTED task #{task_order}.\n\n{verify_msg}"
                            )));
                        }
                        Ok(outcome) => {
                            let policy = ralph_loop_config()
                                .map(|c| c.verification.criteria_policy)
                                .unwrap_or_default();
                            match criteria_verdict(policy, has_criteria, outcome) {
                                CriteriaVerdict::Reject => {
                                    return Ok(ToolResult::error(format!(
                                        "🔒 Ralph Loop REJECTED task #{task_order} (type={task_type_str}): \
                                         it declares acceptance criteria but no verification commands are \
                                         configured for this task type. Under `criteria_policy = \"strict\"` \
                                         a success claim against unverified criteria is refused. Configure \
                                         commands under [verification] in ralph_loop.toml (or remove the \
                                         criteria), then complete again."
                                    )));
                                }
                                CriteriaVerdict::Downgrade => {
                                    criteria_downgraded = true;
                                    tracing::info!(
                                        "Ralph loop: task #{task_order} completion downgraded to Uncertain \
                                         (criteria declared, type '{task_type_str}' has no verification commands)"
                                    );
                                }
                                CriteriaVerdict::Accept => {}
                            }
                        }
                    }
                }

                let (verb, emoji) = {
                    let task = current_plan.get_task_by_order_mut(task_order).unwrap();
                    match action.to_lowercase().as_str() {
                        "skip" => {
                            task.skip(out.clone());
                            ("skipped", "⏭️")
                        }
                        "fail" => {
                            task.fail(out.clone().unwrap_or_else(|| "Task failed.".to_string()));
                            ("failed", "❌")
                        }
                        "success" => {
                            task.complete(out.clone());
                            ("completed", "✅")
                        }
                        other => {
                            return Ok(ToolResult::error(format!(
                                "Unknown action '{other}'. Use 'success', 'fail', or 'skip'."
                            )));
                        }
                    }
                };

                // A resolved task's criteria-goal ends with it (fail keeps
                // the goal: the retry via `start` re-surfaces it).
                let resolved = current_plan.get_task_by_order(task_order).unwrap();
                if verb != "failed" && !resolved.acceptance_criteria.is_empty() {
                    clear_task_goal(context, context.session_id).await;
                }

                let title = current_plan
                    .get_task_by_order(task_order)
                    .unwrap()
                    .title
                    .clone();
                // Epistemic engine (#862): log task outcome as a belief.
                // A criteria-aware downgrade (#870) records Uncertain, not Verified.
                let confidence_override =
                    criteria_downgraded.then_some(super::epistemic::Confidence::Uncertain);
                log_task_outcome_belief(task_order, &title, verb, &out, confidence_override);
                let mut msg = format!("{emoji} Task #{task_order} ({title}) {verb}.");
                if criteria_downgraded {
                    msg.push_str(
                        "\n⚠️ Criteria-aware gate: completion logged as UNCERTAIN, not Verified — \
                         this task declares acceptance criteria but no verification commands ran \
                         for its type. Configure [verification] commands in ralph_loop.toml to \
                         earn a Verified belief.",
                    );
                }
                if let Some(o) = &out {
                    msg.push_str(&format!("\nOutput: {o}"));
                }

                // Auto-start the next executable task and surface its details.
                let next_order = current_plan.next_executable_task().map(|t| t.order);
                if let Some(no) = next_order {
                    current_plan.get_task_by_order_mut(no).unwrap().start();
                    current_plan.status = PlanStatus::Active;
                    let next = current_plan.get_task_by_order(no).unwrap();
                    let details = render_task_details(current_plan, next);
                    msg.push_str(&format!(
                        "\n\n▶️ Started Task #{no}: {}\n{details}",
                        next.title
                    ));
                } else if current_plan
                    .tasks
                    .iter()
                    .all(|t| matches!(t.status, TaskStatus::Completed | TaskStatus::Skipped))
                {
                    msg.push_str(&format!(
                        "\n\n✅ Plan complete. All {} tasks done. The checklist stays \
                         visible until this turn settles, then the plan archives.",
                        current_plan.tasks.len()
                    ));
                } else {
                    msg.push_str(
                        "\n\nNo unblocked task is ready next — remaining tasks are blocked or \
                         failed. Use 'start' with a task_order to retry a failed task.",
                    );
                }
                msg
            }

            // These operations return early (they don't fall through to the
            // shared save below): Approve mutates + saves the plan via
            // try_approve, so re-saving the stale local copy would clobber it;
            // the autonomy ops touch a session marker, not the plan.
            PlanOperation::Approve => {
                if !crate::utils::plan_files::is_plan_autonomy(context.session_id).await {
                    return Ok(ToolResult::error(
                        "Self-approval is off for this session. The user approves the plan via \
                         the Approve button or /execute. If the user told you to proceed \
                         autonomously ('go for it', 'no hand-holding'), call operation \
                         'grant_autonomy' first, then 'approve'."
                            .to_string(),
                    ));
                }
                return match crate::utils::plan_mode::try_approve(context.session_id).await {
                    crate::utils::plan_mode::ApproveOutcome::Refused(msg) => {
                        Ok(ToolResult::error(msg))
                    }
                    crate::utils::plan_mode::ApproveOutcome::SeedTurn { prompt } => {
                        Ok(ToolResult::success(format!(
                            "✅ Plan self-approved (autonomy).\n\n{prompt}"
                        )))
                    }
                };
            }
            PlanOperation::GrantAutonomy => {
                crate::utils::plan_files::set_plan_autonomy(context.session_id, true)
                    .await
                    .map_err(|e| {
                        ToolError::InvalidInput(format!("Failed to grant autonomy: {e}"))
                    })?;
                return Ok(ToolResult::success(
                    "🔓 Autonomy granted for this session: I can self-approve plans with \
                     'approve' instead of waiting for the user's Approve button. Tell the user \
                     it is on and that they can revoke it any time (/plan-auto off, or ask me \
                     to stop)."
                        .to_string(),
                ));
            }
            PlanOperation::RevokeAutonomy => {
                crate::utils::plan_files::set_plan_autonomy(context.session_id, false)
                    .await
                    .map_err(|e| {
                        ToolError::InvalidInput(format!("Failed to revoke autonomy: {e}"))
                    })?;
                return Ok(ToolResult::success(
                    "🔒 Autonomy revoked: plans now require the user's Approve again.".to_string(),
                ));
            }
            PlanOperation::Discard => {
                if matches!(state, crate::utils::plan_files::PlanModeState::NoPlan) {
                    return Ok(ToolResult::error("No live plan to discard.".to_string()));
                }
                // Discarding a live plan is the user's call, not the agent's:
                // a model that can shred its own plan can wiggle out of the
                // review harness mid-gate, or be told to by a malicious
                // message. The user discards via /discard or the plan card's
                // Discard button — both call plan_mode::discard directly and
                // never touch this tool. Sessions with granted plan autonomy
                // keep the old behavior: the user explicitly handed the agent
                // the keys (cron / a2a / hands-off flows).
                if !crate::utils::plan_files::is_plan_autonomy(context.session_id).await {
                    return Ok(ToolResult::error(
                        "Discarding a plan is a user action: ask the user to run /discard \
                         or tap the plan card's Discard button. If the user asked you in \
                         their own words to scrap the plan, relay that request instead of \
                         discarding it yourself."
                            .to_string(),
                    ));
                }
                let reply = if let Some(ref svc) = context.service_context {
                    // Full discard (also clears the plan's goal).
                    crate::utils::plan_mode::discard(context.session_id, svc).await
                } else {
                    // No service context: just remove the plan files.
                    crate::utils::plan_files::discard_plan(context.session_id).await;
                    "🗑️ Plan discarded — back to no plan.".to_string()
                };
                return Ok(ToolResult::success(reply));
            }
            PlanOperation::ShowPlan => {
                return Ok(ToolResult::success(
                    crate::utils::plan_mode::show_plan(context.session_id).await,
                ));
            }
        };

        // Save through the shared store (atomic write, canonical
        // "Editing" / "Active" status strings).
        if let Some(ref current_plan) = plan {
            crate::utils::plan_files::save_plan(current_plan)
                .await
                .map_err(|e| ToolError::InvalidInput(format!("Failed to save plan: {e}")))?;
            tracing::info!(
                "💾 Plan saved to file: {} (status: {:?})",
                plan_file.display(),
                current_plan.status
            );

            // Archive does NOT run here on last complete (ADR 0005 Decision 9):
            // the completing turn keeps its live plan and full all-☑ checklist
            // through delivery. Archive + NoPlan run at turn settle in the
            // shared tool loop (run_tool_loop_inner), which every surface hits,
            // so TUI and Telegram both archive without a surface-specific hook.
        }

        Ok(ToolResult::success(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::tools::epistemic::Confidence;

    #[test]
    fn outcome_confidence_mapping() {
        assert_eq!(task_outcome_confidence("completed"), Confidence::Verified);
        assert_eq!(task_outcome_confidence("failed"), Confidence::Contradicted);
        assert_eq!(task_outcome_confidence("skipped"), Confidence::Uncertain);
    }

    #[test]
    fn outcome_key_stable_across_retries() {
        let k1 = task_outcome_key(3, "Fix the parser");
        let k2 = task_outcome_key(3, "Fix the parser");
        assert_eq!(k1, k2);
    }

    #[test]
    fn outcome_key_differs_by_task() {
        assert_ne!(
            task_outcome_key(3, "Fix the parser"),
            task_outcome_key(4, "Fix the parser")
        );
        assert_ne!(
            task_outcome_key(3, "Fix the parser"),
            task_outcome_key(3, "Write the docs")
        );
    }
}
