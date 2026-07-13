//! Plan Mode Data Structures
//!
//! Core data structures for plan mode, which enables structured task decomposition
//! and controlled execution for complex development tasks.
//!
//! ## Minimal Import Format
//!
//! Only 6 fields required: `title`, `description`, `tasks[]` with `title`, `description`, `task_type`.
//!
//! All other fields are auto-generated on import. See `~/.opencrabs/profiles/<profile>/plans/plan-json-spec.md`
//! for full schema documentation and `~/.opencrabs/profiles/<profile>/plans/coding-plans/` for reference examples.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Task dependency: either a 1-based index into the task list, or a direct UUID reference.
/// This allows both integer indices (easier for LLMs to write) and UUIDs (for explicit references).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "TaskDepDef", into = "TaskDepDef")]
pub enum TaskDep {
    /// 1-based index into the task list. Resolved to Uuid during import.
    Index(usize),
    /// Direct UUID reference to a task.
    Id(Uuid),
}

impl TaskDep {
    /// Convert to UUID, using the provided order-to-UUID mapping if this is an index.
    pub fn to_uuid(&self, order_to_id: &std::collections::HashMap<usize, Uuid>) -> Option<Uuid> {
        match self {
            TaskDep::Id(uuid) => Some(*uuid),
            TaskDep::Index(idx) => order_to_id.get(idx).copied(),
        }
    }

    /// Check if this is a UUID (already resolved)
    pub fn is_uuid(&self) -> bool {
        matches!(self, TaskDep::Id(_))
    }

    /// Get the UUID value if this is a UUID
    pub fn as_uuid(&self) -> Option<Uuid> {
        match self {
            TaskDep::Id(uuid) => Some(*uuid),
            TaskDep::Index(_) => None,
        }
    }
}

/// Serialization format for TaskDep - serializes as just the UUID (index is resolved before serialization)
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum TaskDepDef {
    Uuid(Uuid),
    Index(usize),
}

impl From<TaskDepDef> for TaskDep {
    fn from(def: TaskDepDef) -> Self {
        match def {
            TaskDepDef::Uuid(uuid) => TaskDep::Id(uuid),
            TaskDepDef::Index(idx) => TaskDep::Index(idx),
        }
    }
}

impl From<TaskDep> for TaskDepDef {
    fn from(dep: TaskDep) -> Self {
        match dep {
            TaskDep::Id(uuid) => TaskDepDef::Uuid(uuid),
            TaskDep::Index(idx) => TaskDepDef::Index(idx),
        }
    }
}

/// Custom deserializer for task dependencies - accepts both integer indices (1-based) and UUID strings.
/// Returns Vec<TaskDep> so we preserve the index value for later resolution.
pub fn deserialize_task_deps<'de, D>(deserializer: D) -> Result<Vec<TaskDep>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct TaskDepVisitor;

    impl<'de> serde::de::Visitor<'de> for TaskDepVisitor {
        type Value = Vec<TaskDep>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an array of task indices (1-based integers) or UUID strings")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut deps = Vec::new();
            while let Some(val) = seq.next_element::<serde_json::Value>()? {
                match val {
                    serde_json::Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            if i < 1 {
                                return Err(serde::de::Error::custom("task indices start at 1"));
                            }
                            deps.push(TaskDep::Index(i as usize));
                        } else {
                            return Err(serde::de::Error::custom(
                                "task index must be a positive integer",
                            ));
                        }
                    }
                    serde_json::Value::String(s) => match Uuid::parse_str(&s) {
                        Ok(uuid) => deps.push(TaskDep::Id(uuid)),
                        Err(_) => {
                            return Err(serde::de::Error::custom(format!("invalid UUID: {}", s)));
                        }
                    },
                    _ => {
                        return Err(serde::de::Error::custom(
                            "task dependency must be an integer index or UUID string",
                        ));
                    }
                }
            }
            Ok(deps)
        }
    }

    deserializer.deserialize_seq(TaskDepVisitor)
}

/// Custom serializer for task type — writes lowercase string, matching the deserializer
pub fn serialize_task_type<S>(task_type: &TaskType, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let s = match task_type {
        TaskType::Research => "research",
        TaskType::Edit => "edit",
        TaskType::Create => "create",
        TaskType::Delete => "delete",
        TaskType::Test => "test",
        TaskType::Refactor => "refactor",
        TaskType::Documentation => "documentation",
        TaskType::Configuration => "configuration",
        TaskType::Build => "build",
        TaskType::Other(s) => s.as_str(),
    };
    serializer.serialize_str(s)
}

/// Custom deserializer for task type - case-insensitive. Values outside the
/// documented enum are preserved verbatim as `Other` (lossless fallback) and
/// logged so the mapping stays observable.
pub fn deserialize_task_type<'de, D>(deserializer: D) -> Result<TaskType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "research" => Ok(TaskType::Research),
        "edit" => Ok(TaskType::Edit),
        "create" => Ok(TaskType::Create),
        "delete" => Ok(TaskType::Delete),
        "test" => Ok(TaskType::Test),
        "refactor" => Ok(TaskType::Refactor),
        "documentation" => Ok(TaskType::Documentation),
        "configuration" => Ok(TaskType::Configuration),
        "build" => Ok(TaskType::Build),
        "other" => Ok(TaskType::Other("other".to_string())),
        unknown => {
            tracing::debug!("Unknown task_type '{unknown}' mapped to the 'other' category");
            Ok(TaskType::Other(unknown.to_string()))
        }
    }
}

// Serde default helpers for auto-generated fields
fn default_uuid() -> Uuid {
    Uuid::new_v4()
}
fn default_now() -> DateTime<Utc> {
    Utc::now()
}

/// Plan document containing tasks and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDocument {
    /// Unique plan ID
    #[serde(default = "default_uuid")]
    pub id: Uuid,

    /// Session this plan belongs to
    #[serde(default = "default_uuid")]
    pub session_id: Uuid,

    /// Plan title/goal. Defaults empty so the minimal pre-init Editing
    /// sidecar (flag + empty tasks, no approvable content) parses too.
    #[serde(default)]
    pub title: String,

    /// Detailed description. While post-init Editing, mirrors the full
    /// session `.md` body.
    #[serde(default)]
    pub description: String,

    /// List of tasks to complete
    pub tasks: Vec<PlanTask>,

    /// Context and assumptions
    #[serde(default)]
    pub context: String,

    /// Identified risks and unknowns
    #[serde(default)]
    pub risks: Vec<String>,

    /// Testing strategy and approach
    #[serde(default)]
    pub test_strategy: String,

    /// Technical stack (frameworks, libraries, tools)
    #[serde(default)]
    pub technical_stack: Vec<String>,

    /// Plan status
    #[serde(default)]
    pub status: PlanStatus,

    /// When the plan was created
    #[serde(default = "default_now")]
    pub created_at: DateTime<Utc>,

    /// When the plan was last updated
    #[serde(default = "default_now")]
    pub updated_at: DateTime<Utc>,

    /// When the plan was approved (if applicable). Set on user Approve
    /// (or first `/execute`) on the design track — never auto-set by the
    /// plan tool.
    #[serde(default)]
    pub approved_at: Option<DateTime<Utc>>,

    /// Durable pre-init Editing flag: the user entered Plan-mode intent
    /// (`/plan` / soft-nudge) but `plan init` has not succeeded yet. Lives
    /// on the JSON sidecar (never on `AgentContext`, which is rebuilt every
    /// turn) so it survives restart.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pre_init_editing: bool,
}

impl PlanDocument {
    /// Create a new plan document
    pub fn new(session_id: Uuid, title: String, description: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            session_id,
            title,
            description,
            tasks: Vec::new(),
            context: String::new(),
            risks: Vec::new(),
            test_strategy: String::new(),
            technical_stack: Vec::new(),
            status: PlanStatus::Editing,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            approved_at: None,
            pre_init_editing: false,
        }
    }

    /// Add a task to the plan
    pub fn add_task(&mut self, task: PlanTask) {
        self.tasks.push(task);
        self.updated_at = Utc::now();
    }

    /// Resolve integer index dependencies to UUIDs.
    /// Integer indices (1-based) in dependencies are converted to the UUID of the task at that order.
    /// Note: This requires dependencies to use UUIDs, not integer indices.
    /// Call this after deserialization but before validation.
    pub fn resolve_index_deps(&mut self) {
        use std::collections::HashMap;

        // First pass: build order -> id mapping
        // Use 1-based indexing for tasks
        let mut order_to_id: HashMap<usize, Uuid> = HashMap::new();
        for (idx, task) in self.tasks.iter().enumerate() {
            let order = if task.order > 0 { task.order } else { idx + 1 };
            order_to_id.insert(order, task.id);
        }

        // Second pass: resolve any index dependencies to UUIDs
        for task in &mut self.tasks {
            let resolved: Vec<TaskDep> = task
                .dependencies
                .iter()
                .map(|dep| {
                    match dep {
                        TaskDep::Index(idx) => {
                            // Look up the UUID for this order
                            if let Some(uuid) = order_to_id.get(idx) {
                                TaskDep::Id(*uuid)
                            } else {
                                // Invalid index - keep as-is (will fail validation)
                                TaskDep::Index(*idx)
                            }
                        }
                        TaskDep::Id(_) => dep.clone(),
                    }
                })
                .collect();
            task.dependencies = resolved;
        }
    }

    /// Get tasks in dependency order using topological sort
    /// Returns None if there are circular dependencies
    pub fn tasks_in_order(&self) -> Option<Vec<&PlanTask>> {
        use std::collections::{HashMap, VecDeque};

        // Build dependency graph
        let mut in_degree: HashMap<Uuid, usize> = HashMap::new();
        let mut dependents: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

        // Initialize in-degree for all tasks (count only UUID dependencies, skip indices)
        for task in &self.tasks {
            let uuid_deps: Vec<Uuid> = task
                .dependencies
                .iter()
                .filter_map(|d| d.as_uuid())
                .collect();
            in_degree.insert(task.id, uuid_deps.len());

            // Build reverse dependency map
            for dep_id in &uuid_deps {
                dependents.entry(*dep_id).or_default().push(task.id);
            }
        }

        // Kahn's algorithm for topological sort
        let mut queue: VecDeque<Uuid> = VecDeque::new();

        // Start with tasks that have no dependencies
        for task in &self.tasks {
            if task.dependencies.is_empty() {
                queue.push_back(task.id);
            }
        }

        let mut sorted_ids = Vec::new();

        while let Some(task_id) = queue.pop_front() {
            sorted_ids.push(task_id);

            // Process tasks that depend on this one
            if let Some(deps) = dependents.get(&task_id) {
                for &dependent_id in deps {
                    if let Some(degree) = in_degree.get_mut(&dependent_id) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(dependent_id);
                        }
                    }
                }
            }
        }

        // Check for cycles - if we didn't process all tasks, there's a cycle
        if sorted_ids.len() != self.tasks.len() {
            return None; // Circular dependency detected
        }

        // Convert sorted IDs back to task references
        let task_map: HashMap<Uuid, &PlanTask> = self.tasks.iter().map(|t| (t.id, t)).collect();

        Some(
            sorted_ids
                .iter()
                .filter_map(|id| task_map.get(id).copied())
                .collect(),
        )
    }

    /// Get task by ID
    pub fn get_task(&self, task_id: &Uuid) -> Option<&PlanTask> {
        self.tasks.iter().find(|t| t.id == *task_id)
    }

    /// Get mutable task by ID
    pub fn get_task_mut(&mut self, task_id: &Uuid) -> Option<&mut PlanTask> {
        self.updated_at = Utc::now();
        self.tasks.iter_mut().find(|t| t.id == *task_id)
    }

    /// Count tasks by status
    pub fn count_by_status(&self, status: TaskStatus) -> usize {
        self.tasks.iter().filter(|t| t.status == status).count()
    }

    /// Get progress percentage (0-100)
    pub fn progress_percentage(&self) -> f32 {
        if self.tasks.is_empty() {
            return 0.0;
        }
        let completed = self.count_by_status(TaskStatus::Completed);
        (completed as f32 / self.tasks.len() as f32) * 100.0
    }

    /// Check if all tasks are completed
    pub fn is_complete(&self) -> bool {
        !self.tasks.is_empty()
            && self
                .tasks
                .iter()
                .all(|t| matches!(t.status, TaskStatus::Completed | TaskStatus::Skipped))
    }

    /// Approve the plan: Editing → Active, stamping `approved_at`. Called
    /// on user Approve (or first `/execute`) on the design track — the
    /// plan tool never calls this on `start`.
    pub fn approve(&mut self) {
        self.status = PlanStatus::Active;
        self.approved_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Mark the checklist live (without stamping `approved_at` — that is
    /// user Approve's job on the design track).
    pub fn start_execution(&mut self) {
        self.status = PlanStatus::Active;
        self.updated_at = Utc::now();
    }

    /// Validate task dependencies
    /// Returns Ok(()) if all dependencies are valid, or Err with description of issues
    pub fn validate_dependencies(&self) -> Result<(), String> {
        let task_ids: std::collections::HashSet<Uuid> = self.tasks.iter().map(|t| t.id).collect();

        // Check for invalid task references
        for task in &self.tasks {
            for dep in &task.dependencies {
                if dep.as_uuid().is_some_and(|id| !task_ids.contains(&id)) {
                    return Err(format!(
                        "❌ Invalid Dependency\n\n\
                         Task '{}' (#{}) depends on a task that doesn't exist.\n\n\
                         💡 Fix: Remove this dependency or ensure the referenced task is added first.",
                        task.title, task.order
                    ));
                }
            }
        }

        // Check for circular dependencies using topological sort
        let ordered = self.tasks_in_order();
        if ordered.is_none() {
            // Identify unprocessed tasks (those in the cycle)
            let unprocessed: Vec<&str> = self
                .tasks
                .iter()
                .filter(|task| !task.dependencies.is_empty())
                .map(|task| task.title.as_str())
                .collect();

            return Err(format!(
                "❌ Circular Dependency Detected\n\n\
                 Tasks with dependencies: {}\n\n\
                 💡 Fix: Review the dependency chain and remove circular references.\n\
                 Example: If Task A depends on B, B depends on C, and C depends on A,\n\
                 you need to break one of these dependency links.",
                unprocessed.join(", ")
            ));
        }

        Ok(())
    }

    /// Get the next task to execute (respecting dependencies)
    /// Returns the first pending task whose dependencies are all completed
    pub fn next_executable_task(&self) -> Option<&PlanTask> {
        let completed_ids: std::collections::HashSet<Uuid> = self
            .tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Completed | TaskStatus::Skipped))
            .map(|t| t.id)
            .collect();

        // Find first pending task with all dependencies satisfied
        self.tasks.iter().find(|task| {
            matches!(task.status, TaskStatus::Pending)
                && task
                    .dependencies
                    .iter()
                    .all(|dep| dep.as_uuid().is_some_and(|id| completed_ids.contains(&id)))
        })
    }

    /// Get mutable next executable task
    pub fn next_executable_task_mut(&mut self) -> Option<&mut PlanTask> {
        let completed_ids: std::collections::HashSet<Uuid> = self
            .tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Completed | TaskStatus::Skipped))
            .map(|t| t.id)
            .collect();

        self.updated_at = Utc::now();
        self.tasks.iter_mut().find(|task| {
            matches!(task.status, TaskStatus::Pending)
                && task
                    .dependencies
                    .iter()
                    .all(|dep| dep.as_uuid().is_some_and(|id| completed_ids.contains(&id)))
        })
    }

    /// Get task by order number (1-indexed)
    pub fn get_task_by_order(&self, order: usize) -> Option<&PlanTask> {
        self.tasks.iter().find(|t| t.order == order)
    }

    /// Get mutable task by order number (1-indexed)
    pub fn get_task_by_order_mut(&mut self, order: usize) -> Option<&mut PlanTask> {
        self.updated_at = Utc::now();
        self.tasks.iter_mut().find(|t| t.order == order)
    }

    /// Check if all dependencies for a task are satisfied
    pub fn dependencies_satisfied(&self, task: &PlanTask) -> bool {
        task.dependencies.iter().all(|dep| {
            dep.as_uuid()
                .and_then(|id| self.get_task(&id))
                .map(|dep| matches!(dep.status, TaskStatus::Completed | TaskStatus::Skipped))
                .unwrap_or(false)
        })
    }

    /// Get execution summary for all tasks
    pub fn execution_summary(&self) -> ExecutionSummary {
        let mut summary = ExecutionSummary::default();

        for task in &self.tasks {
            summary.total_tasks += 1;
            match task.status {
                TaskStatus::Completed => summary.completed += 1,
                TaskStatus::Failed => summary.failed += 1,
                TaskStatus::InProgress => summary.in_progress += 1,
                TaskStatus::Pending => summary.pending += 1,
                TaskStatus::Skipped => summary.skipped += 1,
                TaskStatus::Blocked(_) => summary.blocked += 1,
            }
            summary.total_retries += task.retry_count as usize;
        }

        summary
    }

    /// Get tasks that are ready to execute (dependencies satisfied, pending status)
    pub fn ready_tasks(&self) -> Vec<&PlanTask> {
        self.tasks
            .iter()
            .filter(|task| {
                matches!(task.status, TaskStatus::Pending) && self.dependencies_satisfied(task)
            })
            .collect()
    }

    /// Get failed tasks that can be retried
    pub fn retriable_tasks(&self) -> Vec<&PlanTask> {
        self.tasks.iter().filter(|task| task.can_retry()).collect()
    }

    /// Get validation warnings for this plan
    pub fn get_validation_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        // Check for overly complex tasks
        for task in &self.tasks {
            if task.complexity >= 5 {
                warnings.push(format!(
                    "⚠️ Task '{}' has maximum complexity ({}★) - consider breaking it down",
                    task.title, task.complexity
                ));
            }

            // Check for vague task descriptions
            if task.description.len() < 50 {
                warnings.push(format!(
                    "💡 Task '{}' has a brief description ({} chars) - add more detail",
                    task.title,
                    task.description.len()
                ));
            }

            // Check for tasks with no acceptance criteria
            if task.acceptance_criteria.is_empty() {
                warnings.push(format!(
                    "💡 Task '{}' has no acceptance criteria - define success criteria",
                    task.title
                ));
            }
        }

        // Check for plans with too many tasks
        if self.tasks.len() > 20 {
            warnings.push(format!(
                "⚠️ Plan has {} tasks (>20) - consider splitting into smaller plans",
                self.tasks.len()
            ));
        }

        // Check for missing context
        if self.context.is_empty() {
            warnings
                .push("💡 Plan has no context - add environment info or constraints".to_string());
        }

        // Check for missing risks
        if self.risks.is_empty() {
            warnings
                .push("💡 Plan has no identified risks - document potential issues".to_string());
        }

        // Check for missing test strategy
        if self.test_strategy.is_empty() {
            warnings
                .push("💡 Plan has no test strategy - define how to verify success".to_string());
        }

        warnings
    }
}

/// Summary of plan execution
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionSummary {
    pub total_tasks: usize,
    pub completed: usize,
    pub failed: usize,
    pub in_progress: usize,
    pub pending: usize,
    pub skipped: usize,
    pub blocked: usize,
    pub total_retries: usize,
}

/// Status of a plan.
///
/// The live session model is NoPlan / Editing / Active; NoPlan means no
/// plan file exists, so the enum only carries the two live states. On
/// deserialization the seven legacy status strings map onto these two
/// (Draft / PendingApproval / Rejected → Editing; Approved / InProgress →
/// Active). Terminal legacy statuses (Completed archives, Cancelled
/// deletes) are resolved at the file level by
/// `crate::utils::plan_files::load_plan` before this map matters; the
/// serde fallbacks here keep direct parses lossless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanStatus {
    /// Design prose only — no checklist execution. Sub-states (pre-init
    /// vs post-init) are derived from the files on disk, not stored here.
    #[default]
    Editing,
    /// The checklist is live; a design `.md`, if present, is frozen.
    Active,
}

impl Serialize for PlanStatus {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            PlanStatus::Editing => "Editing",
            PlanStatus::Active => "Active",
        })
    }
}

impl<'de> Deserialize<'de> for PlanStatus {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "Active" | "Approved" | "InProgress" | "Completed" => PlanStatus::Active,
            // "Editing", "Draft", "PendingApproval", "Rejected", "Cancelled",
            // and anything unrecognized default to the non-executing state.
            _ => PlanStatus::Editing,
        })
    }
}

impl std::fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanStatus::Editing => write!(f, "Editing"),
            PlanStatus::Active => write!(f, "Active"),
        }
    }
}

/// Individual task within a plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTask {
    /// Unique task ID
    #[serde(default = "default_uuid")]
    pub id: Uuid,

    /// Task number/order
    #[serde(default)]
    pub order: usize,

    /// Task title/summary
    pub title: String,

    /// Detailed description
    pub description: String,

    /// Task type (for categorization)
    #[serde(
        deserialize_with = "deserialize_task_type",
        serialize_with = "serialize_task_type"
    )]
    pub task_type: TaskType,

    /// Dependencies (task IDs or 1-based indices; indices resolved to UUIDs on import)
    #[serde(default, deserialize_with = "deserialize_task_deps")]
    pub dependencies: Vec<TaskDep>,

    /// Estimated complexity (1-5)
    #[serde(default)]
    pub complexity: u8,

    /// Acceptance criteria for task completion
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,

    /// Task status
    #[serde(default)]
    pub status: TaskStatus,

    /// Execution notes/results
    #[serde(default)]
    pub notes: Option<String>,

    /// When task was completed
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,

    /// Number of retry attempts
    #[serde(default)]
    pub retry_count: u8,

    /// Maximum retries allowed
    #[serde(default = "default_max_retries")]
    pub max_retries: u8,

    /// Output artifacts (file paths, generated code, etc.)
    #[serde(default)]
    pub artifacts: Vec<String>,
}

fn default_max_retries() -> u8 {
    3
}

impl PlanTask {
    /// Create a new task
    pub fn new(order: usize, title: String, description: String, task_type: TaskType) -> Self {
        Self {
            id: Uuid::new_v4(),
            order,
            title,
            description,
            task_type,
            dependencies: Vec::new(),
            complexity: 3, // Default medium complexity
            acceptance_criteria: Vec::new(),
            status: TaskStatus::Pending,
            notes: None,
            completed_at: None,
            retry_count: 0,
            max_retries: 3,
            artifacts: Vec::new(),
        }
    }

    /// Mark task as in progress. Idempotent and works from any prior state
    /// (Pending, InProgress, or Failed) so `start` can re-surface a task's
    /// details after a context compaction or to retry a failed task.
    pub fn start(&mut self) {
        self.status = TaskStatus::InProgress;
    }

    /// Add an artifact (file path, generated code, etc.)
    pub fn add_artifact(&mut self, artifact: String) {
        self.artifacts.push(artifact);
    }

    /// Check if task can be retried
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
            && matches!(self.status, TaskStatus::Pending | TaskStatus::Failed)
    }

    /// Complete the task
    pub fn complete(&mut self, notes: Option<String>) {
        self.status = TaskStatus::Completed;
        self.notes = notes;
        self.completed_at = Some(Utc::now());
    }

    /// Mark task as failed
    pub fn fail(&mut self, reason: String) {
        self.status = TaskStatus::Failed;
        self.notes = Some(reason);
    }

    /// Mark task as blocked
    pub fn block(&mut self, reason: String) {
        self.status = TaskStatus::Blocked(reason);
    }

    /// Skip the task
    pub fn skip(&mut self, reason: Option<String>) {
        self.status = TaskStatus::Skipped;
        if let Some(r) = reason {
            self.notes = Some(r);
        }
    }

    /// Get complexity stars (1-5)
    pub fn complexity_stars(&self) -> String {
        let filled = self.complexity.min(5);
        let empty = 5 - filled;
        "★".repeat(filled as usize) + &"☆".repeat(empty as usize)
    }
}

/// Types of tasks
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskType {
    /// Research/exploration
    Research,
    /// File modification
    Edit,
    /// New file creation
    Create,
    /// File deletion
    Delete,
    /// Test creation/modification
    Test,
    /// Refactoring
    Refactor,
    /// Documentation
    Documentation,
    /// Configuration change
    Configuration,
    /// Build/deployment
    Build,
    /// Other
    Other(String),
}

impl std::fmt::Display for TaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskType::Research => write!(f, "Research"),
            TaskType::Edit => write!(f, "Edit"),
            TaskType::Create => write!(f, "Create"),
            TaskType::Delete => write!(f, "Delete"),
            TaskType::Test => write!(f, "Test"),
            TaskType::Refactor => write!(f, "Refactor"),
            TaskType::Documentation => write!(f, "Documentation"),
            TaskType::Configuration => write!(f, "Configuration"),
            TaskType::Build => write!(f, "Build"),
            TaskType::Other(s) => write!(f, "{}", s),
        }
    }
}

/// Status of individual tasks
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TaskStatus {
    /// Not started
    #[default]
    Pending,
    /// Currently being worked on
    InProgress,
    /// Task completed successfully
    Completed,
    /// Task skipped
    Skipped,
    /// Task failed
    Failed,
    /// Task blocked by dependencies or issues
    Blocked(String),
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "Pending"),
            TaskStatus::InProgress => write!(f, "In Progress"),
            TaskStatus::Completed => write!(f, "Completed"),
            TaskStatus::Skipped => write!(f, "Skipped"),
            TaskStatus::Failed => write!(f, "Failed"),
            TaskStatus::Blocked(reason) => write!(f, "Blocked: {}", reason),
        }
    }
}

impl TaskStatus {
    /// Get status icon for UI display
    pub fn icon(&self) -> &str {
        match self {
            TaskStatus::Pending => "⏸️",
            TaskStatus::InProgress => "▶️",
            TaskStatus::Completed => "✅",
            TaskStatus::Skipped => "⏭️",
            TaskStatus::Failed => "❌",
            TaskStatus::Blocked(_) => "🚫",
        }
    }
}
