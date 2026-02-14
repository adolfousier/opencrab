//! Unit tests for Plan Mode
//!
//! Comprehensive test coverage for plan data structures and operations.

#[cfg(test)]
mod tests {
    use crate::tui::plan::*;
    use uuid::Uuid;

    // Helper function to create a test plan
    fn create_test_plan(session_id: Uuid) -> PlanDocument {
        PlanDocument::new(
            session_id,
            "Test Plan".to_string(),
            "A test plan for unit testing".to_string(),
        )
    }

    // Helper function to create a test task
    fn create_test_task(order: usize, title: &str) -> PlanTask {
        PlanTask::new(
            order,
            title.to_string(),
            format!("Description for {}", title),
            TaskType::Edit,
        )
    }

    #[test]
    fn test_plan_document_new() {
        let session_id = Uuid::new_v4();
        let plan = create_test_plan(session_id);

        assert_eq!(plan.session_id, session_id);
        assert_eq!(plan.title, "Test Plan");
        assert_eq!(plan.description, "A test plan for unit testing");
        assert_eq!(plan.status, PlanStatus::Draft);
        assert!(plan.tasks.is_empty());
        assert!(plan.risks.is_empty());
        assert_eq!(plan.context, "");
        assert!(plan.approved_at.is_none());
    }

    #[test]
    fn test_add_task() {
        let mut plan = create_test_plan(Uuid::new_v4());
        let task = create_test_task(1, "Task 1");

        let initial_updated_at = plan.updated_at;
        // Small delay to ensure timestamp changes
        std::thread::sleep(std::time::Duration::from_millis(10));

        plan.add_task(task);

        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].title, "Task 1");
        assert!(plan.updated_at > initial_updated_at);
    }

    #[test]
    fn test_get_task() {
        let mut plan = create_test_plan(Uuid::new_v4());
        let task = create_test_task(1, "Task 1");
        let task_id = task.id;

        plan.add_task(task);

        let found = plan.get_task(&task_id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().title, "Task 1");

        let not_found = plan.get_task(&Uuid::new_v4());
        assert!(not_found.is_none());
    }

    #[test]
    fn test_get_task_mut() {
        let mut plan = create_test_plan(Uuid::new_v4());
        let task = create_test_task(1, "Task 1");
        let task_id = task.id;

        plan.add_task(task);

        let task_mut = plan.get_task_mut(&task_id);
        assert!(task_mut.is_some());

        let task = task_mut.unwrap();
        task.title = "Modified Task".to_string();

        assert_eq!(plan.tasks[0].title, "Modified Task");
    }

    #[test]
    fn test_count_by_status() {
        let mut plan = create_test_plan(Uuid::new_v4());

        let mut task1 = create_test_task(1, "Task 1");
        task1.status = TaskStatus::Pending;

        let mut task2 = create_test_task(2, "Task 2");
        task2.status = TaskStatus::Completed;

        let mut task3 = create_test_task(3, "Task 3");
        task3.status = TaskStatus::Completed;

        plan.add_task(task1);
        plan.add_task(task2);
        plan.add_task(task3);

        assert_eq!(plan.count_by_status(TaskStatus::Pending), 1);
        assert_eq!(plan.count_by_status(TaskStatus::Completed), 2);
        assert_eq!(plan.count_by_status(TaskStatus::Failed), 0);
    }

    #[test]
    fn test_progress_percentage() {
        let mut plan = create_test_plan(Uuid::new_v4());

        // Empty plan
        assert_eq!(plan.progress_percentage(), 0.0);

        // Add tasks
        let mut task1 = create_test_task(1, "Task 1");
        task1.status = TaskStatus::Completed;

        let mut task2 = create_test_task(2, "Task 2");
        task2.status = TaskStatus::Pending;

        let mut task3 = create_test_task(3, "Task 3");
        task3.status = TaskStatus::Completed;

        let mut task4 = create_test_task(4, "Task 4");
        task4.status = TaskStatus::InProgress;

        plan.add_task(task1);
        plan.add_task(task2);
        plan.add_task(task3);
        plan.add_task(task4);

        // 2 out of 4 completed = 50%
        assert_eq!(plan.progress_percentage(), 50.0);
    }

    #[test]
    fn test_is_complete() {
        let mut plan = create_test_plan(Uuid::new_v4());

        // Empty plan is not complete
        assert!(!plan.is_complete());

        // Add incomplete tasks
        let mut task1 = create_test_task(1, "Task 1");
        task1.status = TaskStatus::Pending;
        plan.add_task(task1);

        assert!(!plan.is_complete());

        // Complete the task
        plan.tasks[0].status = TaskStatus::Completed;
        assert!(plan.is_complete());

        // Add a skipped task
        let mut task2 = create_test_task(2, "Task 2");
        task2.status = TaskStatus::Skipped;
        plan.add_task(task2);

        // Still complete (completed + skipped = done)
        assert!(plan.is_complete());

        // Add a failed task
        let mut task3 = create_test_task(3, "Task 3");
        task3.status = TaskStatus::Failed;
        plan.add_task(task3);

        // Not complete anymore
        assert!(!plan.is_complete());
    }

    #[test]
    fn test_plan_state_transitions() {
        let mut plan = create_test_plan(Uuid::new_v4());

        // Draft -> PendingApproval
        assert_eq!(plan.status, PlanStatus::Draft);

        // Approve
        plan.approve();
        assert_eq!(plan.status, PlanStatus::Approved);
        assert!(plan.approved_at.is_some());

        // Start execution
        plan.start_execution();
        assert_eq!(plan.status, PlanStatus::InProgress);

        // Complete
        plan.complete();
        assert_eq!(plan.status, PlanStatus::Completed);
    }

    #[test]
    fn test_plan_rejection() {
        let mut plan = create_test_plan(Uuid::new_v4());

        plan.reject();
        assert_eq!(plan.status, PlanStatus::Rejected);
        assert!(plan.approved_at.is_none());
    }

    #[test]
    fn test_topological_sort_no_dependencies() {
        let mut plan = create_test_plan(Uuid::new_v4());

        let task1 = create_test_task(1, "Task 1");
        let task2 = create_test_task(2, "Task 2");
        let task3 = create_test_task(3, "Task 3");

        plan.add_task(task1);
        plan.add_task(task2);
        plan.add_task(task3);

        let ordered = plan.tasks_in_order();
        assert!(ordered.is_some());

        let ordered = ordered.unwrap();
        assert_eq!(ordered.len(), 3);
    }

    #[test]
    fn test_topological_sort_with_dependencies() {
        let mut plan = create_test_plan(Uuid::new_v4());

        let task1 = create_test_task(1, "Task 1");
        let task1_id = task1.id;

        let task2 = create_test_task(2, "Task 2");
        let task2_id = task2.id;

        let mut task3 = create_test_task(3, "Task 3");
        task3.dependencies.push(task1_id);
        task3.dependencies.push(task2_id);

        plan.add_task(task1);
        plan.add_task(task2);
        plan.add_task(task3);

        let ordered = plan.tasks_in_order();
        assert!(ordered.is_some());

        let ordered = ordered.unwrap();
        assert_eq!(ordered.len(), 3);

        // Task 3 should be last (depends on 1 and 2)
        assert_eq!(ordered[2].title, "Task 3");
    }

    #[test]
    fn test_topological_sort_circular_dependency() {
        let mut plan = create_test_plan(Uuid::new_v4());

        let task1 = create_test_task(1, "Task 1");
        let task1_id = task1.id;

        let mut task2 = create_test_task(2, "Task 2");
        let task2_id = task2.id;
        task2.dependencies.push(task1_id);

        // Create circular dependency: Task 1 depends on Task 2
        let mut task1_modified = task1;
        task1_modified.dependencies.push(task2_id);

        plan.add_task(task1_modified);
        plan.add_task(task2);

        // Should detect cycle and return None
        let ordered = plan.tasks_in_order();
        assert!(ordered.is_none());
    }

    #[test]
    fn test_validate_dependencies_success() {
        let mut plan = create_test_plan(Uuid::new_v4());

        let task1 = create_test_task(1, "Task 1");
        let task1_id = task1.id;

        let mut task2 = create_test_task(2, "Task 2");
        task2.dependencies.push(task1_id);

        plan.add_task(task1);
        plan.add_task(task2);

        let result = plan.validate_dependencies();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_dependencies_invalid_reference() {
        let mut plan = create_test_plan(Uuid::new_v4());

        let mut task1 = create_test_task(1, "Task 1");
        task1.dependencies.push(Uuid::new_v4()); // Non-existent task

        plan.add_task(task1);

        let result = plan.validate_dependencies();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid Dependency"));
    }

    #[test]
    fn test_validate_dependencies_circular() {
        let mut plan = create_test_plan(Uuid::new_v4());

        let task1 = create_test_task(1, "Task 1");
        let task1_id = task1.id;

        let mut task2 = create_test_task(2, "Task 2");
        let task2_id = task2.id;
        task2.dependencies.push(task1_id);

        let mut task1_modified = task1;
        task1_modified.dependencies.push(task2_id);

        plan.add_task(task1_modified);
        plan.add_task(task2);

        let result = plan.validate_dependencies();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Circular Dependency"));
    }

    #[test]
    fn test_task_state_transitions() {
        let mut task = create_test_task(1, "Task 1");

        // Pending -> InProgress
        assert_eq!(task.status, TaskStatus::Pending);
        task.start();
        assert_eq!(task.status, TaskStatus::InProgress);

        // InProgress -> Completed
        task.complete(Some("Task completed successfully".to_string()));
        assert_eq!(task.status, TaskStatus::Completed);
        assert!(task.completed_at.is_some());
        assert_eq!(task.notes, Some("Task completed successfully".to_string()));
    }

    #[test]
    fn test_task_failure() {
        let mut task = create_test_task(1, "Task 1");

        task.fail("Error occurred".to_string());
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(task.notes, Some("Error occurred".to_string()));
        assert!(task.completed_at.is_none());
    }

    #[test]
    fn test_task_blocking() {
        let mut task = create_test_task(1, "Task 1");

        task.block("Waiting for approval".to_string());
        assert!(matches!(task.status, TaskStatus::Blocked(_)));

        if let TaskStatus::Blocked(reason) = &task.status {
            assert_eq!(reason, "Waiting for approval");
        } else {
            panic!("Expected Blocked status");
        }
    }

    #[test]
    fn test_task_skip() {
        let mut task = create_test_task(1, "Task 1");

        task.skip(Some("Not needed".to_string()));
        assert_eq!(task.status, TaskStatus::Skipped);
        assert_eq!(task.notes, Some("Not needed".to_string()));
    }

    #[test]
    fn test_task_complexity_stars() {
        let task1 = PlanTask {
            id: Uuid::new_v4(),
            order: 1,
            title: "Task 1".to_string(),
            description: "Desc".to_string(),
            task_type: TaskType::Edit,
            dependencies: vec![],
            complexity: 1,
            acceptance_criteria: vec![],
            status: TaskStatus::Pending,
            notes: None,
            completed_at: None,
            execution_history: Vec::new(),
            retry_count: 0,
            max_retries: 3,
            artifacts: Vec::new(),
            reflection: None,
        };
        assert_eq!(task1.complexity_stars(), "★☆☆☆☆");

        let task3 = PlanTask {
            complexity: 3,
            ..task1.clone()
        };
        assert_eq!(task3.complexity_stars(), "★★★☆☆");

        let task5 = PlanTask {
            complexity: 5,
            ..task1.clone()
        };
        assert_eq!(task5.complexity_stars(), "★★★★★");

        // Test clamping (> 5)
        let task_high = PlanTask {
            complexity: 10,
            ..task1
        };
        assert_eq!(task_high.complexity_stars(), "★★★★★");
    }

    #[test]
    fn test_task_type_display() {
        assert_eq!(format!("{}", TaskType::Research), "Research");
        assert_eq!(format!("{}", TaskType::Edit), "Edit");
        assert_eq!(format!("{}", TaskType::Create), "Create");
        assert_eq!(format!("{}", TaskType::Delete), "Delete");
        assert_eq!(format!("{}", TaskType::Test), "Test");
        assert_eq!(format!("{}", TaskType::Refactor), "Refactor");
        assert_eq!(format!("{}", TaskType::Documentation), "Documentation");
        assert_eq!(format!("{}", TaskType::Configuration), "Configuration");
        assert_eq!(format!("{}", TaskType::Build), "Build");
        assert_eq!(
            format!("{}", TaskType::Other("Custom".to_string())),
            "Custom"
        );
    }

    #[test]
    fn test_task_status_display() {
        assert_eq!(format!("{}", TaskStatus::Pending), "Pending");
        assert_eq!(format!("{}", TaskStatus::InProgress), "In Progress");
        assert_eq!(format!("{}", TaskStatus::Completed), "Completed");
        assert_eq!(format!("{}", TaskStatus::Skipped), "Skipped");
        assert_eq!(format!("{}", TaskStatus::Failed), "Failed");
        assert_eq!(
            format!("{}", TaskStatus::Blocked("Reason".to_string())),
            "Blocked: Reason"
        );
    }

    #[test]
    fn test_task_status_icons() {
        assert_eq!(TaskStatus::Pending.icon(), "⏸️");
        assert_eq!(TaskStatus::InProgress.icon(), "▶️");
        assert_eq!(TaskStatus::Completed.icon(), "✅");
        assert_eq!(TaskStatus::Skipped.icon(), "⏭️");
        assert_eq!(TaskStatus::Failed.icon(), "❌");
        assert_eq!(TaskStatus::Blocked("".to_string()).icon(), "🚫");
    }

    #[test]
    fn test_plan_status_display() {
        assert_eq!(format!("{}", PlanStatus::Draft), "Draft");
        assert_eq!(
            format!("{}", PlanStatus::PendingApproval),
            "Pending Approval"
        );
        assert_eq!(format!("{}", PlanStatus::Approved), "Approved");
        assert_eq!(format!("{}", PlanStatus::Rejected), "Rejected");
        assert_eq!(format!("{}", PlanStatus::InProgress), "In Progress");
        assert_eq!(format!("{}", PlanStatus::Completed), "Completed");
        assert_eq!(format!("{}", PlanStatus::Cancelled), "Cancelled");
    }

    #[test]
    fn test_complex_dependency_chain() {
        let mut plan = create_test_plan(Uuid::new_v4());

        // Create a complex dependency graph:
        // Task 1 (no deps)
        // Task 2 (depends on 1)
        // Task 3 (depends on 1)
        // Task 4 (depends on 2 and 3)
        // Task 5 (depends on 4)

        let task1 = create_test_task(1, "Task 1");
        let task1_id = task1.id;

        let mut task2 = create_test_task(2, "Task 2");
        task2.dependencies.push(task1_id);
        let task2_id = task2.id;

        let mut task3 = create_test_task(3, "Task 3");
        task3.dependencies.push(task1_id);
        let task3_id = task3.id;

        let mut task4 = create_test_task(4, "Task 4");
        task4.dependencies.push(task2_id);
        task4.dependencies.push(task3_id);
        let task4_id = task4.id;

        let mut task5 = create_test_task(5, "Task 5");
        task5.dependencies.push(task4_id);

        plan.add_task(task1);
        plan.add_task(task2);
        plan.add_task(task3);
        plan.add_task(task4);
        plan.add_task(task5);

        // Validate dependencies
        let result = plan.validate_dependencies();
        assert!(result.is_ok());

        // Get topological order
        let ordered = plan.tasks_in_order();
        assert!(ordered.is_some());

        let ordered = ordered.unwrap();
        assert_eq!(ordered.len(), 5);

        // Task 1 should be first
        assert_eq!(ordered[0].title, "Task 1");

        // Task 5 should be last
        assert_eq!(ordered[4].title, "Task 5");

        // Task 4 should come after Task 2 and Task 3
        let task2_pos = ordered.iter().position(|t| t.title == "Task 2").unwrap();
        let task3_pos = ordered.iter().position(|t| t.title == "Task 3").unwrap();
        let task4_pos = ordered.iter().position(|t| t.title == "Task 4").unwrap();

        assert!(task4_pos > task2_pos);
        assert!(task4_pos > task3_pos);
    }
}
