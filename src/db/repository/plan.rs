//! Plan Repository
//!
//! Database operations for plans and plan tasks.

use crate::db::Pool;
use crate::db::database::interact_err;
use crate::db::models::{Plan, PlanTask};
use crate::tui::plan::{PlanDocument, PlanStatus, TaskDep, TaskStatus, TaskType};
use anyhow::{Context, Result};
use rusqlite::params;
use uuid::Uuid;

/// Repository for plan operations
#[derive(Clone)]
pub struct PlanRepository {
    pool: Pool,
}

impl PlanRepository {
    /// Create a new plan repository
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Find plan by ID
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<PlanDocument>> {
        let id_str = id.to_string();
        let plan = self
            .pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                conn.prepare_cached("SELECT * FROM plans WHERE id = ?1")?
                    .query_row(params![id_str], Plan::from_row)
                    .optional()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to find plan")?;

        let Some(plan) = plan else {
            return Ok(None);
        };

        // Fetch associated tasks
        let tasks = self.find_tasks_by_plan_id(id).await?;

        // Convert database models to domain models
        Ok(Some(self.plan_from_db(plan, tasks)?))
    }

    /// Find all plans for a session
    pub async fn find_by_session_id(&self, session_id: Uuid) -> Result<Vec<PlanDocument>> {
        let sid = session_id.to_string();
        let plans = self
            .pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT * FROM plans WHERE session_id = ?1 ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map(params![sid], Plan::from_row)?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to find plans by session")?;

        let mut result = Vec::new();
        for plan in plans {
            let tasks = self.find_tasks_by_plan_id(plan.id).await?;
            result.push(self.plan_from_db(plan, tasks)?);
        }

        Ok(result)
    }

    /// Find tasks for a plan
    async fn find_tasks_by_plan_id(&self, plan_id: Uuid) -> Result<Vec<PlanTask>> {
        let pid = plan_id.to_string();
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT * FROM plan_tasks WHERE plan_id = ?1 ORDER BY task_order ASC",
                )?;
                let rows = stmt.query_map(params![pid], PlanTask::from_row)?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to find plan tasks")
    }

    /// Create a new plan with tasks
    pub async fn create(&self, plan: &PlanDocument) -> Result<()> {
        let (db_plan, db_tasks) = self.plan_to_db(plan)?;

        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                let tx = conn.transaction()?;

                tx.execute(
                    "INSERT INTO plans (id, session_id, title, description, context, risks,
                                     test_strategy, technical_stack, status, created_at, updated_at, approved_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        db_plan.id.to_string(),
                        db_plan.session_id.to_string(),
                        db_plan.title,
                        db_plan.description,
                        db_plan.context,
                        db_plan.risks,
                        db_plan.test_strategy,
                        db_plan.technical_stack,
                        db_plan.status,
                        db_plan.created_at.timestamp(),
                        db_plan.updated_at.timestamp(),
                        db_plan.approved_at.map(|dt| dt.timestamp()),
                    ],
                )?;

                for task in &db_tasks {
                    tx.execute(
                        "INSERT INTO plan_tasks (id, plan_id, task_order, title, description,
                                               task_type, dependencies, complexity, acceptance_criteria,
                                               status, notes, completed_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                        params![
                            task.id.to_string(),
                            task.plan_id.to_string(),
                            task.task_order,
                            task.title,
                            task.description,
                            task.task_type,
                            task.dependencies,
                            task.complexity,
                            task.acceptance_criteria,
                            task.status,
                            task.notes,
                            task.completed_at.map(|dt| dt.timestamp()),
                        ],
                    )?;
                }

                tx.commit()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to create plan")?;

        tracing::debug!("Created plan: {}", plan.id);
        Ok(())
    }

    /// Update an existing plan
    pub async fn update(&self, plan: &PlanDocument) -> Result<()> {
        let (db_plan, db_tasks) = self.plan_to_db(plan)?;

        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| {
                let tx = conn.transaction()?;

                tx.execute(
                    "UPDATE plans
                     SET title = ?1, description = ?2, context = ?3, risks = ?4,
                         test_strategy = ?5, technical_stack = ?6,
                         status = ?7, updated_at = ?8, approved_at = ?9
                     WHERE id = ?10",
                    params![
                        db_plan.title,
                        db_plan.description,
                        db_plan.context,
                        db_plan.risks,
                        db_plan.test_strategy,
                        db_plan.technical_stack,
                        db_plan.status,
                        db_plan.updated_at.timestamp(),
                        db_plan.approved_at.map(|dt| dt.timestamp()),
                        db_plan.id.to_string(),
                    ],
                )?;

                // Delete existing tasks and re-insert
                tx.execute(
                    "DELETE FROM plan_tasks WHERE plan_id = ?1",
                    params![db_plan.id.to_string()],
                )?;

                for task in &db_tasks {
                    tx.execute(
                        "INSERT INTO plan_tasks (id, plan_id, task_order, title, description,
                                               task_type, dependencies, complexity, acceptance_criteria,
                                               status, notes, completed_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                        params![
                            task.id.to_string(),
                            task.plan_id.to_string(),
                            task.task_order,
                            task.title,
                            task.description,
                            task.task_type,
                            task.dependencies,
                            task.complexity,
                            task.acceptance_criteria,
                            task.status,
                            task.notes,
                            task.completed_at.map(|dt| dt.timestamp()),
                        ],
                    )?;
                }

                tx.commit()
            })
            .await
            .map_err(interact_err)?
            .context("Failed to update plan")?;

        tracing::debug!("Updated plan: {}", plan.id);
        Ok(())
    }

    /// Delete a plan and all its tasks
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        let id_str = id.to_string();
        // Tasks will be deleted automatically via CASCADE
        self.pool
            .get()
            .await
            .context("Failed to get connection")?
            .interact(move |conn| conn.execute("DELETE FROM plans WHERE id = ?1", params![id_str]))
            .await
            .map_err(interact_err)?
            .context("Failed to delete plan")?;

        tracing::debug!("Deleted plan: {}", id);
        Ok(())
    }

    /// Convert database models to domain model
    fn plan_from_db(&self, db_plan: Plan, db_tasks: Vec<PlanTask>) -> Result<PlanDocument> {
        let risks: Vec<String> =
            serde_json::from_str(&db_plan.risks).context("Failed to parse risks JSON")?;
        let technical_stack: Vec<String> = serde_json::from_str(&db_plan.technical_stack)
            .context("Failed to parse technical_stack JSON")?;

        let status = self.parse_plan_status(&db_plan.status)?;

        let mut tasks = Vec::new();
        for db_task in db_tasks {
            tasks.push(self.task_from_db(db_task)?);
        }

        Ok(PlanDocument {
            id: db_plan.id,
            session_id: db_plan.session_id,
            title: db_plan.title,
            description: db_plan.description,
            tasks,
            context: db_plan.context,
            risks,
            test_strategy: db_plan.test_strategy,
            technical_stack,
            status,
            created_at: db_plan.created_at,
            updated_at: db_plan.updated_at,
            approved_at: db_plan.approved_at,
        })
    }

    /// Convert database task to domain task
    fn task_from_db(&self, db_task: PlanTask) -> Result<crate::tui::plan::PlanTask> {
        let dependencies: Vec<TaskDep> = serde_json::from_str(&db_task.dependencies)
            .context("Failed to parse dependencies JSON")?;
        let acceptance_criteria: Vec<String> =
            serde_json::from_str(&db_task.acceptance_criteria)
                .context("Failed to parse acceptance_criteria JSON")?;

        let task_type = self.parse_task_type(&db_task.task_type)?;
        let status = self.parse_task_status(&db_task.status)?;

        Ok(crate::tui::plan::PlanTask {
            id: db_task.id,
            order: db_task.task_order as usize,
            title: db_task.title,
            description: db_task.description,
            task_type,
            dependencies,
            complexity: db_task.complexity as u8,
            acceptance_criteria,
            status,
            notes: db_task.notes,
            completed_at: db_task.completed_at,
            retry_count: 0,
            max_retries: 3,
            artifacts: Vec::new(),
        })
    }

    /// Convert domain model to database models
    fn plan_to_db(&self, plan: &PlanDocument) -> Result<(Plan, Vec<PlanTask>)> {
        let risks = serde_json::to_string(&plan.risks).context("Failed to serialize risks")?;
        let technical_stack = serde_json::to_string(&plan.technical_stack)
            .context("Failed to serialize technical_stack")?;

        let db_plan = Plan {
            id: plan.id,
            session_id: plan.session_id,
            title: plan.title.clone(),
            description: plan.description.clone(),
            context: plan.context.clone(),
            risks,
            test_strategy: plan.test_strategy.clone(),
            technical_stack,
            status: self.format_plan_status(&plan.status),
            created_at: plan.created_at,
            updated_at: plan.updated_at,
            approved_at: plan.approved_at,
        };

        let mut db_tasks = Vec::new();
        for task in &plan.tasks {
            db_tasks.push(self.task_to_db(task, plan.id)?);
        }

        Ok((db_plan, db_tasks))
    }

    /// Convert domain task to database task
    fn task_to_db(&self, task: &crate::tui::plan::PlanTask, plan_id: Uuid) -> Result<PlanTask> {
        let dependencies = serde_json::to_string(&task.dependencies)
            .context("Failed to serialize dependencies")?;
        let acceptance_criteria = serde_json::to_string(&task.acceptance_criteria)
            .context("Failed to serialize acceptance_criteria")?;

        Ok(PlanTask {
            id: task.id,
            plan_id,
            task_order: task.order as i32,
            title: task.title.clone(),
            description: task.description.clone(),
            task_type: self.format_task_type(&task.task_type),
            dependencies,
            complexity: task.complexity as i32,
            acceptance_criteria,
            status: self.format_task_status(&task.status),
            notes: task.notes.clone(),
            completed_at: task.completed_at,
        })
    }

    /// Parse plan status from string
    fn parse_plan_status(&self, status: &str) -> Result<PlanStatus> {
        Ok(match status {
            "Draft" => PlanStatus::Draft,
            "PendingApproval" => PlanStatus::PendingApproval,
            "Approved" => PlanStatus::Approved,
            "Rejected" => PlanStatus::Rejected,
            "InProgress" => PlanStatus::InProgress,
            "Completed" => PlanStatus::Completed,
            "Cancelled" => PlanStatus::Cancelled,
            _ => anyhow::bail!("Invalid plan status: {}", status),
        })
    }

    /// Format plan status to string
    fn format_plan_status(&self, status: &PlanStatus) -> String {
        match status {
            PlanStatus::Draft => "Draft",
            PlanStatus::PendingApproval => "PendingApproval",
            PlanStatus::Approved => "Approved",
            PlanStatus::Rejected => "Rejected",
            PlanStatus::InProgress => "InProgress",
            PlanStatus::Completed => "Completed",
            PlanStatus::Cancelled => "Cancelled",
        }
        .to_string()
    }

    /// Parse task type from string
    fn parse_task_type(&self, task_type: &str) -> Result<TaskType> {
        Ok(match task_type {
            "Research" => TaskType::Research,
            "Edit" => TaskType::Edit,
            "Create" => TaskType::Create,
            "Delete" => TaskType::Delete,
            "Test" => TaskType::Test,
            "Refactor" => TaskType::Refactor,
            "Documentation" => TaskType::Documentation,
            "Configuration" => TaskType::Configuration,
            "Build" => TaskType::Build,
            other => TaskType::Other(other.to_string()),
        })
    }

    /// Format task type to string
    fn format_task_type(&self, task_type: &TaskType) -> String {
        match task_type {
            TaskType::Research => "Research",
            TaskType::Edit => "Edit",
            TaskType::Create => "Create",
            TaskType::Delete => "Delete",
            TaskType::Test => "Test",
            TaskType::Refactor => "Refactor",
            TaskType::Documentation => "Documentation",
            TaskType::Configuration => "Configuration",
            TaskType::Build => "Build",
            TaskType::Other(s) => s,
        }
        .to_string()
    }

    /// Parse task status from string
    fn parse_task_status(&self, status: &str) -> Result<TaskStatus> {
        if let Some(reason) = status.strip_prefix("Blocked:") {
            return Ok(TaskStatus::Blocked(reason.trim().to_string()));
        }

        Ok(match status {
            "Pending" => TaskStatus::Pending,
            "InProgress" => TaskStatus::InProgress,
            "Completed" => TaskStatus::Completed,
            "Skipped" => TaskStatus::Skipped,
            "Failed" => TaskStatus::Failed,
            _ => anyhow::bail!("Invalid task status: {}", status),
        })
    }

    /// Format task status to string
    fn format_task_status(&self, status: &TaskStatus) -> String {
        match status {
            TaskStatus::Pending => "Pending".to_string(),
            TaskStatus::InProgress => "InProgress".to_string(),
            TaskStatus::Completed => "Completed".to_string(),
            TaskStatus::Skipped => "Skipped".to_string(),
            TaskStatus::Failed => "Failed".to_string(),
            TaskStatus::Blocked(reason) => format!("Blocked:{}", reason),
        }
    }
}

/// Extension trait for rusqlite to add `.optional()` to query results
trait OptionalExt<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}

impl<T> OptionalExt<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
