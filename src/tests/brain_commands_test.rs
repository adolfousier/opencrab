use crate::brain::commands::*;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_load_nonexistent() {
    let loader = CommandLoader::new(PathBuf::from("/nonexistent/commands.toml"));
    let commands = loader.load();
    assert!(commands.is_empty());
}

#[test]
fn test_save_and_load_toml() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("commands.toml");
    let loader = CommandLoader::new(path);

    let commands = vec![
        UserCommand {
            name: "/deploy".to_string(),
            description: "Deploy to staging".to_string(),
            action: "prompt".to_string(),
            prompt: "Run deploy.sh".to_string(),
        },
        UserCommand {
            name: "/test".to_string(),
            description: "Run tests".to_string(),
            action: "prompt".to_string(),
            prompt: "Run cargo test".to_string(),
        },
    ];

    loader.save(&commands).unwrap();
    let loaded = loader.load();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].name, "/deploy");
    assert_eq!(loaded[1].name, "/test");
}

#[test]
fn test_json_migration() {
    let dir = TempDir::new().unwrap();
    let json_path = dir.path().join("commands.json");
    let toml_path = dir.path().join("commands.toml");

    // Write legacy JSON
    let commands = vec![UserCommand {
        name: "/legacy".to_string(),
        description: "Legacy command".to_string(),
        action: "prompt".to_string(),
        prompt: "do legacy stuff".to_string(),
    }];
    let json = serde_json::to_string_pretty(&commands).unwrap();
    std::fs::write(&json_path, json).unwrap();

    // Load should find JSON and auto-migrate
    let loader = CommandLoader::new(toml_path.clone());
    let loaded = loader.load();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "/legacy");

    // TOML file should now exist
    assert!(toml_path.exists());

    // Loading again should use TOML
    let loaded2 = loader.load();
    assert_eq!(loaded2.len(), 1);
}

#[test]
fn test_add_command() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("commands.toml");
    let loader = CommandLoader::new(path);

    loader
        .add_command(UserCommand {
            name: "/first".to_string(),
            description: "First".to_string(),
            action: "prompt".to_string(),
            prompt: "first".to_string(),
        })
        .unwrap();

    loader
        .add_command(UserCommand {
            name: "/second".to_string(),
            description: "Second".to_string(),
            action: "prompt".to_string(),
            prompt: "second".to_string(),
        })
        .unwrap();

    let loaded = loader.load();
    assert_eq!(loaded.len(), 2);

    // Update existing
    loader
        .add_command(UserCommand {
            name: "/first".to_string(),
            description: "Updated first".to_string(),
            action: "prompt".to_string(),
            prompt: "updated".to_string(),
        })
        .unwrap();

    let loaded = loader.load();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].description, "Updated first");
}

#[test]
fn test_remove_command() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("commands.toml");
    let loader = CommandLoader::new(path);

    let commands = vec![
        UserCommand {
            name: "/keep".to_string(),
            description: "Keep".to_string(),
            action: "prompt".to_string(),
            prompt: "keep".to_string(),
        },
        UserCommand {
            name: "/remove".to_string(),
            description: "Remove".to_string(),
            action: "prompt".to_string(),
            prompt: "remove".to_string(),
        },
    ];
    loader.save(&commands).unwrap();

    let removed = loader.remove_command("/remove").unwrap();
    assert!(removed);

    let loaded = loader.load();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "/keep");

    let removed = loader.remove_command("/nonexistent").unwrap();
    assert!(!removed);
}

#[test]
fn test_commands_section() {
    let builtin = vec![("/help", "Show help"), ("/models", "Switch model")];
    let user = vec![UserCommand {
        name: "/deploy".to_string(),
        description: "Deploy".to_string(),
        action: "prompt".to_string(),
        prompt: "deploy".to_string(),
    }];

    let section = CommandLoader::commands_section(&builtin, &user);
    assert!(section.contains("/help"));
    assert!(section.contains("/deploy"));
    assert!(section.contains("User-defined"));
}
