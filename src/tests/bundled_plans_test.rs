use crate::brain::plans;
use crate::brain::plans::files;
use crate::tui::plan::{PlanDocument, PlanStatus, TaskDep, TaskStatus};

fn embedded_content(rel_path: &str) -> &'static str {
    match rel_path {
        "plan-json-spec.md" => include_str!("../docs/reference/plans/plan-json-spec.md"),
        "coding-plans/python-fast.json" => {
            include_str!("../docs/reference/plans/coding-plans/python-fast.json")
        }
        "coding-plans/python-medium.json" => {
            include_str!("../docs/reference/plans/coding-plans/python-medium.json")
        }
        "coding-plans/python-full.json" => {
            include_str!("../docs/reference/plans/coding-plans/python-full.json")
        }
        "coding-plans/rust-fast.json" => {
            include_str!("../docs/reference/plans/coding-plans/rust-fast.json")
        }
        "coding-plans/rust-medium.json" => {
            include_str!("../docs/reference/plans/coding-plans/rust-medium.json")
        }
        "coding-plans/rust-full.json" => {
            include_str!("../docs/reference/plans/coding-plans/rust-full.json")
        }
        "coding-plans/sample-minimal-plan.json" => {
            include_str!("../docs/reference/plans/coding-plans/sample-minimal-plan.json")
        }
        _ => panic!("unknown bundled file: {}", rel_path),
    }
}

#[test]
fn test_embedded_content_all_files() {
    let file_list = vec![
        "plan-json-spec.md",
        "coding-plans/python-fast.json",
        "coding-plans/python-medium.json",
        "coding-plans/python-full.json",
        "coding-plans/rust-fast.json",
        "coding-plans/rust-medium.json",
        "coding-plans/rust-full.json",
        "coding-plans/sample-minimal-plan.json",
    ];
    for f in file_list {
        let content = embedded_content(f);
        assert!(!content.is_empty(), "embedded file {f} should not be empty");
    }
}

#[test]
#[should_panic(expected = "unknown bundled file")]
fn test_embedded_content_unknown_file() {
    embedded_content("nonexistent.json");
}

#[test]
fn test_minimal_plan_deserializes() {
    let content = embedded_content("coding-plans/sample-minimal-plan.json");
    let plan: PlanDocument =
        serde_json::from_str(content).expect("sample-minimal-plan.json must deserialize");
    assert_eq!(plan.title, "Sample: Minimal Plan (Import-Ready Format)");
    assert_eq!(plan.tasks.len(), 4);
    assert!(!plan.id.is_nil(), "id should be auto-generated UUID");
    assert!(
        !plan.session_id.is_nil(),
        "session_id should be auto-generated"
    );
    assert!(
        plan.tasks.iter().all(|t| !t.id.is_nil()),
        "task ids should be auto-generated"
    );
}

#[test]
fn test_numeric_dependencies_deserialize() {
    let content = embedded_content("coding-plans/python-fast.json");
    let plan: PlanDocument =
        serde_json::from_str(content).expect("python-fast.json must deserialize");
    assert_eq!(plan.title, "Python Fast Path");
    assert_eq!(plan.tasks.len(), 3);
    assert!(plan.tasks[0].dependencies.is_empty());
    assert_eq!(plan.tasks[1].dependencies.len(), 1);
    assert!(
        !plan.tasks[1].dependencies[0].is_uuid(),
        "should be Index, not UUID"
    );
    assert_eq!(plan.tasks[2].dependencies.len(), 1);
    assert!(
        !plan.tasks[2].dependencies[0].is_uuid(),
        "should be Index, not UUID"
    );
}

#[test]
fn test_resolve_index_deps_on_bundled_plan() {
    let content = embedded_content("coding-plans/python-fast.json");
    let mut plan: PlanDocument = serde_json::from_str(content).unwrap();
    plan.resolve_index_deps();
    for task in &plan.tasks {
        for dep in &task.dependencies {
            assert!(dep.is_uuid(), "dep should be resolved to UUID, got {dep:?}");
        }
    }
}

#[test]
fn test_all_bundled_plans_deserialize() {
    let paths = vec![
        "python-fast",
        "python-medium",
        "python-full",
        "rust-fast",
        "rust-medium",
        "rust-full",
        "sample-minimal-plan",
    ];
    for name in paths {
        let content = embedded_content(&format!("coding-plans/{name}.json"));
        let plan: PlanDocument = serde_json::from_str(content)
            .unwrap_or_else(|e| panic!("{name}.json failed to deserialize: {e}"));
        assert!(!plan.title.is_empty(), "{name}: title must not be empty");
        assert!(
            !plan.tasks.is_empty(),
            "{name}: must have at least one task"
        );
    }
}

#[test]
fn test_minimal_format_with_no_optional_fields() {
    let json = r#"{
        "title": "Quick Fix",
        "description": "Fix the bug",
        "tasks": [
            {"title": "Fix it", "description": "Do the thing", "task_type": "edit"}
        ]
    }"#;
    let plan: PlanDocument = serde_json::from_str(json).expect("minimal format must deserialize");
    assert_eq!(plan.title, "Quick Fix");
    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].order, 0, "order defaults to 0 when omitted");
    assert!(plan.tasks[0].dependencies.is_empty());
    assert!(plan.tasks[0].acceptance_criteria.is_empty());
    assert!(plan.risks.is_empty());
    assert!(plan.context.is_empty());
}

#[test]
fn test_plan_status_default() {
    assert_eq!(PlanStatus::default(), PlanStatus::Editing);
}

#[test]
fn test_task_status_default() {
    assert_eq!(TaskStatus::default(), TaskStatus::Pending);
}

#[test]
fn test_task_dep_id_variant() {
    let uuid = uuid::Uuid::new_v4();
    let dep = TaskDep::Id(uuid);

    assert!(dep.is_uuid());
    assert_eq!(dep.as_uuid(), Some(uuid));
}

#[test]
fn test_task_dep_index_variant() {
    let dep = TaskDep::Index(1);

    assert!(!dep.is_uuid(), "Index is not a UUID");
    assert_eq!(dep.as_uuid(), None);
}

#[test]
fn test_task_dep_deserialize_numeric() {
    let json = r#"[1, 2, 3]"#;
    let deps: Vec<TaskDep> = serde_json::from_str(json).unwrap();
    assert_eq!(deps.len(), 3);
    assert!(
        deps.iter().all(|d| !d.is_uuid()),
        "numeric deps should deserialize as Index"
    );
}

#[test]
fn test_task_dep_deserialize_uuid() {
    let uuid1 = uuid::Uuid::new_v4();
    let uuid2 = uuid::Uuid::new_v4();
    let json = format!(r#"["{}", "{}"]"#, uuid1, uuid2);
    let deps: Vec<TaskDep> = serde_json::from_str(&json).unwrap();
    assert_eq!(deps.len(), 2);
    assert!(deps.iter().all(|d| d.is_uuid()));
    assert_eq!(deps[0].as_uuid(), Some(uuid1));
    assert_eq!(deps[1].as_uuid(), Some(uuid2));
}

#[test]
fn test_task_dep_deserialize_mixed() {
    let uuid = uuid::Uuid::new_v4();
    let json = format!(r#"[1, "{}"]"#, uuid);
    let deps: Vec<TaskDep> = serde_json::from_str(&json).unwrap();
    assert_eq!(deps.len(), 2);
    assert!(!deps[0].is_uuid(), "numeric should be Index");
    assert!(deps[1].is_uuid(), "string UUID should be Id");
}

#[test]
fn test_plan_document_default_values() {
    let json = r#"{"title": "Test", "description": "Test plan", "tasks": []}"#;
    let plan: PlanDocument = serde_json::from_str(json).unwrap();
    assert!(!plan.id.is_nil(), "id should be auto-generated");
    assert!(
        !plan.session_id.is_nil(),
        "session_id should be auto-generated"
    );
    assert_eq!(plan.status, PlanStatus::Editing);
    assert_eq!(plan.context, "");
    assert!(plan.risks.is_empty());
    assert!(plan.technical_stack.is_empty());
}

#[test]
fn test_resolve_index_deps_mixed_format() {
    let uuid = uuid::Uuid::new_v4();
    let json = format!(
        r#"{{
        "title": "Mixed Deps",
        "description": "test",
        "tasks": [
            {{"title": "A", "description": "", "task_type": "research", "order": 1}},
            {{"title": "B", "description": "", "task_type": "build", "order": 2, "dependencies": [1]}},
            {{"title": "C", "description": "", "task_type": "test", "dependencies": ["{}"]}}
        ]
    }}"#,
        uuid
    );
    let mut plan: PlanDocument = serde_json::from_str(&json).unwrap();
    plan.resolve_index_deps();

    assert!(
        plan.tasks[1].dependencies[0].is_uuid(),
        "numeric dep 1 should resolve to task A's UUID"
    );
    assert_eq!(
        plan.tasks[1].dependencies[0].as_uuid(),
        Some(plan.tasks[0].id)
    );
    assert!(
        plan.tasks[2].dependencies[0].is_uuid(),
        "UUID dep should remain UUID"
    );
    assert_eq!(plan.tasks[2].dependencies[0].as_uuid(), Some(uuid));
}

#[test]
fn test_task_dep_to_uuid() {
    use std::collections::HashMap;

    let uuid = uuid::Uuid::new_v4();
    let id_dep = TaskDep::Id(uuid);
    let idx_dep = TaskDep::Index(1);

    let mut order_map = HashMap::new();
    order_map.insert(1, uuid);

    assert_eq!(id_dep.to_uuid(&order_map), Some(uuid));
    assert_eq!(idx_dep.to_uuid(&order_map), Some(uuid));

    let missing_idx = TaskDep::Index(99);
    assert_eq!(
        missing_idx.to_uuid(&order_map),
        None,
        "missing index should return None"
    );
}

#[test]
fn test_plans_dir_path() {
    let dir = plans::plans_dir();
    assert!(
        dir.to_string_lossy().contains("plans"),
        "plans_dir should point to a 'plans' subdirectory"
    );
}

#[test]
fn test_coding_plan_path_returns_json() {
    let path = files::coding_plan_path("python-fast");
    assert!(
        path.to_string_lossy().ends_with(".json"),
        "coding_plan_path should return .json file"
    );
}

#[test]
fn test_all_coding_plan_paths_count() {
    let paths = files::all_coding_plan_paths();
    assert_eq!(paths.len(), 7, "should have 7 bundled coding plans");
}
