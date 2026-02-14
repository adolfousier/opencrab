//! Plan Mode Data Structures
//!
//! Core data structures for plan mode, which enables structured task decomposition
//! and controlled execution for complex development tasks.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Plan document containing tasks and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDocument {
    /// Unique plan ID
    pub id: Uuid,

    /// Session this plan belongs to
    pub session_id: Uuid,

    /// Plan title/goal
    pub title: String,

    /// Detailed description
    pub description: String,

    /// List of tasks to complete
    pub tasks: Vec<PlanTask>,

    /// Context and assumptions
    pub context: String,

    /// Identified risks and unknowns
    pub risks: Vec<String>,

    /// Testing strategy and approach
    pub test_strategy: String,

    /// Technical stack (frameworks, libraries, tools)
    pub technical_stack: Vec<String>,

    /// Plan status
    pub status: PlanStatus,

    /// When the plan was created
    pub created_at: DateTime<Utc>,

    /// When the plan was last updated
    pub updated_at: DateTime<Utc>,

    /// When the plan was approved (if applicable)
    pub approved_at: Option<DateTime<Utc>>,
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
            status: PlanStatus::Draft,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            approved_at: None,
        }
    }

    /// Add a task to the plan
    pub fn add_task(&mut self, task: PlanTask) {
        self.tasks.push(task);
        self.updated_at = Utc::now();
    }

    /// Get tasks in dependency order using topological sort
    /// Returns None if there are circular dependencies
    pub fn tasks_in_order(&self) -> Option<Vec<&PlanTask>> {
        use std::collections::{HashMap, VecDeque};

        // Build dependency graph
        let mut in_degree: HashMap<Uuid, usize> = HashMap::new();
        let mut dependents: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

        // Initialize in-degree for all tasks
        for task in &self.tasks {
            in_degree.insert(task.id, task.dependencies.len());

            // Build reverse dependency map
            for &dep_id in &task.dependencies {
                dependents.entry(dep_id).or_default().push(task.id);
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

    /// Approve the plan
    pub fn approve(&mut self) {
        self.status = PlanStatus::Approved;
        self.approved_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Reject the plan
    pub fn reject(&mut self) {
        self.status = PlanStatus::Rejected;
        self.updated_at = Utc::now();
    }

    /// Mark plan as in progress
    pub fn start_execution(&mut self) {
        self.status = PlanStatus::InProgress;
        self.updated_at = Utc::now();
    }

    /// Mark plan as completed
    pub fn complete(&mut self) {
        self.status = PlanStatus::Completed;
        self.updated_at = Utc::now();
    }

    /// Validate task dependencies
    /// Returns Ok(()) if all dependencies are valid, or Err with description of issues
    pub fn validate_dependencies(&self) -> Result<(), String> {
        let task_ids: std::collections::HashSet<Uuid> = self.tasks.iter().map(|t| t.id).collect();

        // Check for invalid task references
        for task in &self.tasks {
            for &dep_id in &task.dependencies {
                if !task_ids.contains(&dep_id) {
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
                    .all(|dep| completed_ids.contains(dep))
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
                    .all(|dep| completed_ids.contains(dep))
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
        task.dependencies.iter().all(|dep_id| {
            self.get_task(dep_id)
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
            summary.total_tool_calls += task
                .execution_history
                .iter()
                .map(|e| e.tools_called.len())
                .sum::<usize>();
        }

        summary.success_rate = if summary.completed + summary.failed > 0 {
            (summary.completed as f32 / (summary.completed + summary.failed) as f32) * 100.0
        } else {
            0.0
        };

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
    pub total_tool_calls: usize,
    pub success_rate: f32,
}

/// Status of a plan
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlanStatus {
    /// Plan is being drafted
    Draft,
    /// Plan is ready for review
    PendingApproval,
    /// Plan was approved by user
    Approved,
    /// Plan was rejected, needs revision
    Rejected,
    /// Plan is being executed
    InProgress,
    /// All tasks completed
    Completed,
    /// Plan was cancelled
    Cancelled,
}

impl std::fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanStatus::Draft => write!(f, "Draft"),
            PlanStatus::PendingApproval => write!(f, "Pending Approval"),
            PlanStatus::Approved => write!(f, "Approved"),
            PlanStatus::Rejected => write!(f, "Rejected"),
            PlanStatus::InProgress => write!(f, "In Progress"),
            PlanStatus::Completed => write!(f, "Completed"),
            PlanStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// Individual task within a plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTask {
    /// Unique task ID
    pub id: Uuid,

    /// Task number/order
    pub order: usize,

    /// Task title/summary
    pub title: String,

    /// Detailed description
    pub description: String,

    /// Task type (for categorization)
    pub task_type: TaskType,

    /// Dependencies (task IDs that must complete first)
    pub dependencies: Vec<Uuid>,

    /// Estimated complexity (1-5)
    pub complexity: u8,

    /// Acceptance criteria for task completion
    pub acceptance_criteria: Vec<String>,

    /// Task status
    pub status: TaskStatus,

    /// Execution notes/results
    pub notes: Option<String>,

    /// When task was completed
    pub completed_at: Option<DateTime<Utc>>,

    /// Execution history (for plan-and-execute pattern)
    #[serde(default)]
    pub execution_history: Vec<TaskExecution>,

    /// Number of retry attempts
    #[serde(default)]
    pub retry_count: u8,

    /// Maximum retries allowed
    #[serde(default = "default_max_retries")]
    pub max_retries: u8,

    /// Output artifacts (file paths, generated code, etc.)
    #[serde(default)]
    pub artifacts: Vec<String>,

    /// Reflection notes from LLM after execution
    #[serde(default)]
    pub reflection: Option<String>,
}

fn default_max_retries() -> u8 {
    3
}

/// Record of a single execution attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecution {
    /// When this execution attempt started
    pub started_at: DateTime<Utc>,

    /// When this execution attempt ended
    pub ended_at: Option<DateTime<Utc>>,

    /// Tools called during this execution
    pub tools_called: Vec<ToolCall>,

    /// Output/result of this execution
    pub output: Option<String>,

    /// Error if execution failed
    pub error: Option<String>,

    /// Whether this attempt was successful
    pub success: bool,
}

/// Record of a tool call during task execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool name
    pub tool_name: String,

    /// Tool input (JSON)
    pub input: serde_json::Value,

    /// Tool output
    pub output: Option<String>,

    /// Whether the call succeeded
    pub success: bool,

    /// Timestamp
    pub timestamp: DateTime<Utc>,
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
            execution_history: Vec::new(),
            retry_count: 0,
            max_retries: 3,
            artifacts: Vec::new(),
            reflection: None,
        }
    }

    /// Mark task as in progress
    pub fn start(&mut self) {
        self.status = TaskStatus::InProgress;
    }

    /// Start a new execution attempt
    pub fn start_execution(&mut self) -> &mut TaskExecution {
        self.status = TaskStatus::InProgress;
        let execution = TaskExecution {
            started_at: Utc::now(),
            ended_at: None,
            tools_called: Vec::new(),
            output: None,
            error: None,
            success: false,
        };
        self.execution_history.push(execution);
        self.execution_history.last_mut().unwrap()
    }

    /// Record a tool call in the current execution
    pub fn record_tool_call(&mut self, tool_call: ToolCall) {
        if let Some(execution) = self.execution_history.last_mut() {
            execution.tools_called.push(tool_call);
        }
    }

    /// Complete the current execution attempt
    pub fn complete_execution(&mut self, output: String, success: bool) {
        if let Some(execution) = self.execution_history.last_mut() {
            execution.ended_at = Some(Utc::now());
            execution.output = Some(output.clone());
            execution.success = success;
        }

        if success {
            self.status = TaskStatus::Completed;
            self.notes = Some(output);
            self.completed_at = Some(Utc::now());
        } else {
            self.retry_count += 1;
            if self.retry_count >= self.max_retries {
                self.status = TaskStatus::Failed;
            } else {
                self.status = TaskStatus::Pending; // Ready for retry
            }
        }
    }

    /// Mark execution as failed with error
    pub fn fail_execution(&mut self, error: String) {
        if let Some(execution) = self.execution_history.last_mut() {
            execution.ended_at = Some(Utc::now());
            execution.error = Some(error.clone());
            execution.success = false;
        }

        self.retry_count += 1;
        if self.retry_count >= self.max_retries {
            self.status = TaskStatus::Failed;
            self.notes = Some(format!(
                "Failed after {} attempts: {}",
                self.retry_count, error
            ));
        } else {
            self.status = TaskStatus::Pending;
        }
    }

    /// Add reflection notes after execution
    pub fn add_reflection(&mut self, reflection: String) {
        self.reflection = Some(reflection);
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

    /// Get the last execution attempt
    pub fn last_execution(&self) -> Option<&TaskExecution> {
        self.execution_history.last()
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    /// Not started
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

#[cfg(test)]
#[path = "plan_tests.rs"]
mod plan_tests;
