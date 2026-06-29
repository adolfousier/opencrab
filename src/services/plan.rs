//! Plan Service
//!
//! Business logic for plan management operations.

use crate::db::repository::PlanRepository;
use crate::services::ServiceContext;
use crate::tui::plan::{PlanDocument, PlanStatus, TaskStatus};
use anyhow::Result;
use uuid::Uuid;

/// Validation warnings for a plan
#[derive(Debug, Clone)]
pub struct PlanValidationWarning {
    pub severity: WarningSeverity,
    pub message: String,
    pub suggestion: Option<String>,
}

/// Severity level for validation warnings
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningSeverity {
    Info,
    Warning,
    Error,
}

/// Plan statistics for analytics
#[derive(Debug, Clone)]
pub struct PlanStatistics {
    pub total_plans: usize,
    pub completed_plans: usize,
    pub in_progress_plans: usize,
    pub average_tasks_per_plan: f64,
    pub average_completion_rate: f64,
    pub total_tasks_executed: usize,
    pub total_tasks_succeeded: usize,
    pub total_tasks_failed: usize,
}

/// Service for plan operations
#[derive(Clone)]
pub struct PlanService {
    repo: PlanRepository,
}

impl PlanService {
    /// Create a new plan service
    pub fn new(context: ServiceContext) -> Self {
        Self {
            repo: PlanRepository::new(context.pool()),
        }
    }

    /// Find plan by ID
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<PlanDocument>> {
        self.repo.find_by_id(id).await
    }

    /// Find all plans for a session
    pub async fn find_by_session_id(&self, session_id: Uuid) -> Result<Vec<PlanDocument>> {
        self.repo.find_by_session_id(session_id).await
    }

    /// Get the most recent plan for a session
    pub async fn get_most_recent_plan(&self, session_id: Uuid) -> Result<Option<PlanDocument>> {
        let plans = self.find_by_session_id(session_id).await?;
        // Plans are already sorted by updated_at DESC in the repository
        Ok(plans.into_iter().next())
    }

    /// Create a new plan
    pub async fn create(&self, plan: &PlanDocument) -> Result<()> {
        self.repo.create(plan).await
    }

    /// Update an existing plan
    pub async fn update(&self, plan: &PlanDocument) -> Result<()> {
        self.repo.update(plan).await
    }

    /// Delete a plan
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        self.repo.delete(id).await
    }

    /// Save plan to JSON file as backup
    /// This is a fallback/export mechanism
    pub async fn export_to_json(
        &self,
        plan: &PlanDocument,
        file_path: &std::path::Path,
    ) -> Result<()> {
        let json = serde_json::to_string_pretty(plan)?;

        // Atomic write: write to temp file, then rename
        let temp_file = file_path.with_extension("tmp");
        tokio::fs::write(&temp_file, &json).await?;
        tokio::fs::rename(&temp_file, file_path).await?;

        Ok(())
    }

    /// Import plan from JSON file
    /// Used for migrating existing JSON plans to database
    pub async fn import_from_json(&self, file_path: &std::path::Path) -> Result<PlanDocument> {
        let content = tokio::fs::read_to_string(file_path).await?;
        let plan: PlanDocument = serde_json::from_str(&content)?;
        Ok(plan)
    }

    /// Validate a plan and return warnings
    pub fn validate_plan(&self, plan: &PlanDocument) -> Vec<PlanValidationWarning> {
        let mut warnings = Vec::new();

        // Check for overly complex tasks
        for task in &plan.tasks {
            if task.complexity >= 5 {
                warnings.push(PlanValidationWarning {
                    severity: WarningSeverity::Warning,
                    message: format!(
                        "Task '{}' has maximum complexity ({}★)",
                        task.title, task.complexity
                    ),
                    suggestion: Some(
                        "Consider breaking this task into smaller, more manageable subtasks."
                            .to_string(),
                    ),
                });
            }

            // Check for vague task descriptions
            if task.description.len() < 50 {
                warnings.push(PlanValidationWarning {
                    severity: WarningSeverity::Info,
                    message: format!(
                        "Task '{}' has a brief description ({} chars)",
                        task.title,
                        task.description.len()
                    ),
                    suggestion: Some(
                        "Add more detailed implementation steps for better clarity.".to_string(),
                    ),
                });
            }

            // Check for tasks with no acceptance criteria
            if task.acceptance_criteria.is_empty() {
                warnings.push(PlanValidationWarning {
                    severity: WarningSeverity::Info,
                    message: format!("Task '{}' has no acceptance criteria", task.title),
                    suggestion: Some(
                        "Define clear success criteria to know when the task is complete."
                            .to_string(),
                    ),
                });
            }
        }

        // Check for plans with too many tasks
        if plan.tasks.len() > 20 {
            warnings.push(PlanValidationWarning {
                severity: WarningSeverity::Warning,
                message: format!("Plan has {} tasks (>20)", plan.tasks.len()),
                suggestion: Some(
                    "Consider splitting into multiple smaller plans for better manageability."
                        .to_string(),
                ),
            });
        }

        // Check for empty plan
        if plan.tasks.is_empty() {
            warnings.push(PlanValidationWarning {
                severity: WarningSeverity::Error,
                message: "Plan has no tasks".to_string(),
                suggestion: Some("Add at least one task before finalizing the plan.".to_string()),
            });
        }

        // Check for missing context
        if plan.context.is_empty() {
            warnings.push(PlanValidationWarning {
                severity: WarningSeverity::Info,
                message: "Plan has no context information".to_string(),
                suggestion: Some(
                    "Add context about the environment, constraints, or assumptions.".to_string(),
                ),
            });
        }

        // Check for missing risks
        if plan.risks.is_empty() {
            warnings.push(PlanValidationWarning {
                severity: WarningSeverity::Info,
                message: "Plan has no identified risks".to_string(),
                suggestion: Some(
                    "Consider what could go wrong and document potential risks.".to_string(),
                ),
            });
        }

        warnings
    }

    /// Get plan history for a session (all plans, sorted by creation date)
    pub async fn get_plan_history(&self, session_id: Uuid) -> Result<Vec<PlanDocument>> {
        self.find_by_session_id(session_id).await
    }

    /// Get completed plans for a session
    pub async fn get_completed_plans(&self, session_id: Uuid) -> Result<Vec<PlanDocument>> {
        let plans = self.find_by_session_id(session_id).await?;
        Ok(plans
            .into_iter()
            .filter(|p| p.status == PlanStatus::Completed)
            .collect())
    }

    /// Get active (in-progress or pending approval) plans for a session
    pub async fn get_active_plans(&self, session_id: Uuid) -> Result<Vec<PlanDocument>> {
        let plans = self.find_by_session_id(session_id).await?;
        Ok(plans
            .into_iter()
            .filter(|p| {
                matches!(
                    p.status,
                    PlanStatus::InProgress | PlanStatus::PendingApproval | PlanStatus::Approved
                )
            })
            .collect())
    }

    /// Get statistics for plans in a session
    pub async fn get_statistics(&self, session_id: Uuid) -> Result<PlanStatistics> {
        let plans = self.find_by_session_id(session_id).await?;

        let total_plans = plans.len();
        let completed_plans = plans
            .iter()
            .filter(|p| p.status == PlanStatus::Completed)
            .count();
        let in_progress_plans = plans
            .iter()
            .filter(|p| p.status == PlanStatus::InProgress)
            .count();

        let total_tasks: usize = plans.iter().map(|p| p.tasks.len()).sum();
        let average_tasks_per_plan = if total_plans > 0 {
            total_tasks as f64 / total_plans as f64
        } else {
            0.0
        };

        let mut total_completion_rate = 0.0;
        let mut plans_with_tasks = 0;
        let mut total_tasks_succeeded = 0;
        let mut total_tasks_failed = 0;

        for plan in &plans {
            if !plan.tasks.is_empty() {
                total_completion_rate += plan.progress_percentage() as f64;
                plans_with_tasks += 1;
            }

            for task in &plan.tasks {
                match task.status {
                    TaskStatus::Completed => total_tasks_succeeded += 1,
                    TaskStatus::Failed => total_tasks_failed += 1,
                    _ => {}
                }
            }
        }

        let average_completion_rate = if plans_with_tasks > 0 {
            total_completion_rate / plans_with_tasks as f64
        } else {
            0.0
        };

        Ok(PlanStatistics {
            total_plans,
            completed_plans,
            in_progress_plans,
            average_tasks_per_plan,
            average_completion_rate,
            total_tasks_executed: total_tasks_succeeded + total_tasks_failed,
            total_tasks_succeeded,
            total_tasks_failed,
        })
    }
}

#[cfg(test)]
#[path = "../tests/services_plan_test.rs"]
mod tests;
