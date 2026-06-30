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
    /// Create a new plan (from a title, optionally with inline tasks) OR
    /// import one from a JSON file. Replaces any existing plan in the session.
    Init {
        /// Plan title (create mode). One of `title` / `file_path` is required.
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        context: String,
        #[serde(default)]
        risks: Vec<String>,
        #[serde(default)]
        test_strategy: String,
        #[serde(default)]
        technical_stack: Vec<String>,
        /// Import mode: absolute path to a plan JSON file on disk. Takes
        /// precedence over `title` when both are present.
        #[serde(default)]
        file_path: Option<String>,
        /// Optional inline task definitions (create mode) — plan + tasks in one call.
        #[serde(default)]
        tasks: Vec<InlineTask>,
    },
    /// Add a task to the current plan (appended at the end).
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
    /// Find and start the next task — or a specific one via `task_order`.
    /// Returns full task details. Idempotent on an in-progress task (re-surfaces
    /// its details after a compaction); resets a failed task for retry. The
    /// first start auto-approves the plan for execution.
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
/// to `Other`.
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
        other => TaskType::Other(other.to_string()),
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
    if !description.is_empty() {
        validate_string(&description, MAX_DESCRIPTION_LENGTH, "Task description")?;
    }
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
        "Manage a structured task plan for multi-step work. FOUR operations: \
         `init` (create a plan from a title — optionally with inline `tasks` — or import one from a \
         JSON `file_path`), `add_task` (append a task), `start` (begin the next task, or a specific \
         one via `task_order`), and `complete` (finish a task and auto-start the next). \
         \n\nFLOW: init → add_task… → start → (do the work) → complete → (auto-starts next) → complete → … \
         `start` and `complete` return the FULL task details (description, acceptance criteria, \
         dependencies), so the plan doubles as durable memory across context compactions — call \
         `start` with no args to re-surface the in-progress task's details after a compaction. \
         \n\nWHEN TO USE: call `plan` BEFORE starting any task with 3+ distinct steps, dependencies \
         between steps, that touches multiple files, or that spans multiple commits; when the user \
         asks for a plan/roadmap; when a request describes >2 deliverables; or when the user will \
         step away while you work. Skip planning for trivial single-tool answers. \
         \n\nMARK PROGRESS AS YOU GO: after each task's work is VERIFIED done (command exited 0, file \
         written, tests/clippy pass), immediately call `complete` for it before moving on. The TUI \
         progress widget counts only completed tasks, so a stale 0/N while work is done means a \
         `complete` was skipped, not that progress is tracked some other way. \
         \n\nDETAILS: `start` is idempotent on an in-progress task and resets a failed task for retry; \
         the first `start` auto-approves the plan. `complete` takes action=\"success\" (default), \
         \"fail\", or \"skip\". Day-of-week of dependencies is by task order number (1-based). \
         \n\nBUNDLED REFERENCE PLANS for import: source at `src/docs/reference/plans/` (embedded), \
         runtime at `~/.opencrabs/profiles/<profile>/plans/`. See `coding-plans/rust-fast.json` etc. \
         and `plan-json-spec.md`. \
         \n\nRE-TESTING AFTER BUG FIX: plans are forward-only — a completed task stays completed. If a \
         later task introduces a bug an earlier test would catch, `add_task` a new test task rather \
         than re-opening the completed one."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["init", "add_task", "start", "complete"],
                    "description": "init (create/import a plan), add_task (append a task), start (begin next/specific task), complete (finish a task, auto-start next)"
                },
                "title": {
                    "type": "string",
                    "description": "Plan title (init create mode) or task title (add_task)"
                },
                "description": {
                    "type": "string",
                    "description": "Plan or task description (init / add_task)"
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
                    "description": "Optional inline task definitions for init create mode — each: {title, description?, task_type?, complexity?, dependencies?, acceptance_criteria?}. Lets you create the plan and all tasks in one call."
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

        // Load existing plan with security checks
        let mut plan: Option<PlanDocument> = if plan_file.exists() {
            // Security: Check file size before reading
            let metadata = tokio::fs::metadata(&plan_file)
                .await
                .map_err(ToolError::Io)?;

            if metadata.len() > MAX_PLAN_FILE_SIZE {
                return Err(ToolError::InvalidInput(format!(
                    "Plan file too large: {} bytes (max: {} bytes)",
                    metadata.len(),
                    MAX_PLAN_FILE_SIZE
                )));
            }

            let content = tokio::fs::read_to_string(&plan_file)
                .await
                .map_err(ToolError::Io)?;

            Some(serde_json::from_str(&content).map_err(|e| {
                ToolError::InvalidInput(format!("Failed to parse plan file: {}", e))
            })?)
        } else {
            None
        };

        let result = match operation {
            PlanOperation::Init {
                title,
                description,
                context: ctx,
                risks,
                test_strategy,
                technical_stack,
                file_path,
                tasks,
            } => {
                if let Some(path) = file_path {
                    // ===== import mode =====
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
                    imported.status = PlanStatus::Draft;
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
                    for task in &mut imported.tasks {
                        let new_id = old_to_new[&task.id];
                        task.id = new_id;
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
                    let description = description.unwrap_or_default();
                    if !description.is_empty() {
                        validate_string(&description, MAX_DESCRIPTION_LENGTH, "Plan description")?;
                    }
                    if !ctx.is_empty() {
                        validate_string(&ctx, MAX_CONTEXT_LENGTH, "Plan context")?;
                    }

                    if let Some(existing_plan) = plan.as_ref() {
                        tracing::info!(
                            "Replacing existing plan '{}' ({} tasks) with new plan '{}'",
                            existing_plan.title,
                            existing_plan.tasks.len(),
                            title
                        );
                    }

                    let mut new_plan =
                        PlanDocument::new(context.session_id, title.clone(), description);
                    new_plan.context = ctx;
                    new_plan.risks = risks;
                    new_plan.test_strategy = test_strategy;
                    new_plan.technical_stack = technical_stack;
                    new_plan.status = PlanStatus::Draft;

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

                    let count = new_plan.tasks.len();
                    let list = render_task_list(&new_plan);
                    plan = Some(new_plan);

                    if count == 0 {
                        format!(
                            "📋 Created plan: {title} (no tasks yet)\n\n\
                             Add tasks with 'add_task', then 'start' to begin."
                        )
                    } else {
                        format!(
                            "📋 Created plan: {title} ({count} tasks)\n\n{list}\n\n\
                             Call 'start' to begin — it returns the first task's full details."
                        )
                    }
                }
            }

            PlanOperation::AddTask {
                title,
                description,
                task_type,
                dependencies,
                complexity,
                acceptance_criteria,
            } => {
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
                let current_plan = plan.as_mut().ok_or_else(|| {
                    ToolError::InvalidInput(
                        "No active plan. Create one with 'init' first.".to_string(),
                    )
                })?;
                if current_plan.tasks.is_empty() {
                    return Ok(ToolResult::error(
                        "Plan has no tasks yet. Add tasks with 'add_task' first.".to_string(),
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
                            current_plan.complete();
                            format!(
                                "✅ Plan complete. All {} tasks done.",
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
                            // start() sets InProgress (also resets a Failed task for retry).
                            current_plan.get_task_by_order_mut(order).unwrap().start();
                            current_plan.status = PlanStatus::InProgress;
                            // Auto-approve on first start (transition from Draft)
                            if current_plan.approved_at.is_none() {
                                current_plan.approved_at = Some(Utc::now());
                            }
                        }

                        let done = current_plan
                            .tasks
                            .iter()
                            .filter(|t| matches!(t.status, TaskStatus::Completed))
                            .count();
                        let total = current_plan.tasks.len();
                        let task = current_plan.get_task_by_order(order).unwrap();
                        let details = render_task_details(current_plan, task);
                        if already_done {
                            format!(
                                "Task #{order}: {} — already {status:?}.\n\n{details}\n\n\
                                 Progress: {done}/{total} done.",
                                task.title
                            )
                        } else {
                            format!(
                                "▶️ Task #{order}: {}\n\n{details}\n\n\
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

                let title = current_plan
                    .get_task_by_order(task_order)
                    .unwrap()
                    .title
                    .clone();
                let mut msg = format!("{emoji} Task #{task_order} ({title}) {verb}.");
                if let Some(o) = &out {
                    msg.push_str(&format!("\nOutput: {o}"));
                }

                // Auto-start the next executable task and surface its details.
                let next_order = current_plan.next_executable_task().map(|t| t.order);
                if let Some(no) = next_order {
                    current_plan.get_task_by_order_mut(no).unwrap().start();
                    current_plan.status = PlanStatus::InProgress;
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
                    current_plan.complete();
                    msg.push_str(&format!(
                        "\n\n✅ Plan complete. All {} tasks done.",
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
        };

        // Save plan to file with atomic write
        if let Some(ref current_plan) = plan {
            let json = serde_json::to_string_pretty(current_plan)
                .map_err(|e| ToolError::InvalidInput(format!("Failed to serialize plan: {}", e)))?;

            // Atomic write: write to temp file, then rename
            let temp_file = plan_file.with_extension("tmp");

            // Write to temp file
            tokio::fs::write(&temp_file, &json)
                .await
                .map_err(ToolError::Io)?;

            // Atomic rename (ensures consistency even if interrupted)
            tokio::fs::rename(&temp_file, &plan_file)
                .await
                .map_err(ToolError::Io)?;

            tracing::info!(
                "💾 Plan saved to file: {} (status: {:?})",
                plan_file.display(),
                current_plan.status
            );

            // Verify file was written correctly
            if plan_file.exists() {
                match tokio::fs::read_to_string(&plan_file).await {
                    Ok(content) => match serde_json::from_str::<PlanDocument>(&content) {
                        Ok(saved_plan) => {
                            tracing::debug!(
                                "✅ Verified saved plan: status={:?}, tasks={}",
                                saved_plan.status,
                                saved_plan.tasks.len()
                            );

                            if saved_plan.status != current_plan.status {
                                tracing::error!(
                                    "❌ Status mismatch! Expected {:?}, got {:?}",
                                    current_plan.status,
                                    saved_plan.status
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!("❌ Failed to parse saved plan: {}", e);
                        }
                    },
                    Err(e) => {
                        tracing::error!("❌ Failed to read saved plan: {}", e);
                    }
                }
            } else {
                tracing::error!("❌ Plan file does not exist after save!");
            }
        }

        Ok(ToolResult::success(result))
    }
}
