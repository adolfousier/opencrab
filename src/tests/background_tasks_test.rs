//! #722 phase 2: the background-task manager runs a command detached and, on
//! completion, enqueues a system QueuedUserMessage into the originating session.

use crate::brain::agent::service::MessageEnqueueCallback;
use crate::brain::agent::service::QueuedUserMessage;
use crate::brain::agent::service::background_tasks::{
    BackgroundTaskManager, CmdResult, completion_message, is_known_long, short_label, tail_lines,
};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[test]
fn tail_keeps_last_n_lines() {
    let text = (1..=100)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let tail = tail_lines(&text, 3);
    assert_eq!(tail, "98\n99\n100");
    // Fewer lines than n -> whole text.
    assert_eq!(tail_lines("a\nb", 10), "a\nb");
}

#[test]
fn completion_message_reflects_success_and_failure() {
    let ok = completion_message(
        "cargo test",
        "cargo test --all-features",
        &CmdResult {
            success: true,
            code: 0,
            output: "test result: ok. 5 passed".into(),
        },
    );
    assert!(ok.context_text.contains("exit 0 (success)"));
    assert!(ok.context_text.contains("cargo test --all-features"));
    assert!(ok.context_text.contains("Do not re-run"));
    assert!(ok.display_text.contains("finished"));

    let fail = completion_message(
        "build",
        "cargo build",
        &CmdResult {
            success: false,
            code: 101,
            output: "error[E0001]".into(),
        },
    );
    assert!(fail.context_text.contains("exit 101 (failure)"));
    assert!(fail.display_text.contains("failed"));
}

#[test]
fn known_long_matches_the_named_cases() {
    assert!(is_known_long("cargo test --all-features"));
    assert!(is_known_long("cd ~/proj && cargo build --release"));
    assert!(is_known_long("npx remotion render Main out/x.mp4"));
    assert!(is_known_long("gh run watch 12345"));
    // Ordinary quick commands are not backgrounded.
    assert!(!is_known_long("ls -la"));
    assert!(!is_known_long("git status"));
    assert!(!is_known_long("cat README.md"));
}

#[test]
fn short_label_takes_the_command_after_cd() {
    assert_eq!(short_label("cd ~/proj && cargo test"), "cargo test");
    assert_eq!(short_label("cargo build"), "cargo build");
}

#[tokio::test]
async fn spawn_command_enqueues_on_completion() {
    #[allow(clippy::type_complexity)]
    let recorded: Arc<Mutex<Vec<(Uuid, QueuedUserMessage)>>> = Arc::new(Mutex::new(Vec::new()));
    let rec = recorded.clone();
    let enqueue: MessageEnqueueCallback = Arc::new(move |sid, msg| {
        rec.lock().unwrap().push((sid, msg));
    });

    let mgr = Arc::new(BackgroundTaskManager::new(enqueue));
    let sid = Uuid::new_v4();
    let cwd = std::env::temp_dir();

    mgr.clone().spawn_command(
        sid,
        cwd,
        "echo probe".to_string(),
        "echo BG_DONE_MARKER".to_string(),
    );

    // Wait (bounded) for the detached command to finish and enqueue.
    let mut waited = 0;
    while recorded.lock().unwrap().is_empty() && waited < 50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        waited += 1;
    }

    let r = recorded.lock().unwrap();
    assert_eq!(r.len(), 1, "completion should have enqueued exactly once");
    assert_eq!(r[0].0, sid);
    assert!(r[0].1.context_text.contains("BG_DONE_MARKER"));
    assert!(r[0].1.context_text.contains("exit 0 (success)"));
    // Running count drops back to zero after completion.
    assert_eq!(mgr.running_for(sid), 0);
}

// A marker that is merely MENTIONED is not a long command: writing a report
// that names a build step, grepping for the phrase, or feeding an edit script
// a source line that contains one. Detaching those ends the turn on a command
// that finishes in milliseconds, and each completion comes back as an injected
// message that starts another turn.
#[test]
fn mentioning_a_marker_in_data_is_not_a_long_command() {
    // Heredoc body: the report text names the command, the shell never runs it.
    assert!(!is_known_long(
        "cat > /tmp/note.md <<'EOF'\nRun cargo test --all-features to verify.\nEOF"
    ));
    // A marker alone on a body line must not read as a command position.
    assert!(!is_known_long(
        "cat >> src/tests/x.rs <<'RUST'\ncargo test --lib\nRUST"
    ));
    // Quoted grep pattern.
    assert!(!is_known_long(
        r#"grep -c -i "cargo test\|cargo clippy" src/tests/x.rs"#
    ));
    // Interpreter heredoc whose body inserts the phrase into a source file.
    assert!(!is_known_long(
        "python3 - <<'PY'\ns = \"cargo clippy --all-features\"\nPY"
    ));
    // Here-strings are not heredocs, and this one is still only data.
    assert!(!is_known_long("echo \"cargo build --release\""));
}

// Command position still detaches, including the shapes the earlier substring
// match got right by accident.
#[test]
fn markers_in_command_position_still_background() {
    assert!(is_known_long("cargo test --all-features --lib plan_tool"));
    assert!(is_known_long("cd ~/proj && cargo build --release"));
    assert!(is_known_long("cargo fmt; echo done; cargo clippy --locked"));
    assert!(is_known_long("W=$(cargo clippy --all-features 2>&1)"));
    assert!(is_known_long("cargo fmt\ncargo test --lib"));
    assert!(is_known_long("for f in a b; do cargo test --lib; done"));
}
