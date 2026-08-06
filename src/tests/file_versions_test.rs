//! Regression (#954): a whole-file write cannot silently overwrite another
//! agent's change.
//!
//! Several agents share one working directory by design — every channel gets
//! its own service, `spawn_agent` hands children the parent's directory, and
//! `team_create` launches a team into it with `max_concurrent` defaulting to 4.
//! `write_file` replaces a file wholesale using content composed from a read in
//! an earlier tool call, so anything written in between was lost with nothing
//! reported.
//!
//! The guard refuses rather than serialises: an agent can re-read and redo its
//! change, but it cannot notice that its write destroyed someone else's.

use crate::brain::tools::file_versions::{forget_session, is_stale_write, record};
use uuid::Uuid;

/// A real file on disk — `check` canonicalizes, so the path has to exist.
fn temp_file(contents: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("shared.rs");
    std::fs::write(&path, contents).expect("seed file");
    (dir, path)
}

#[test]
fn creating_a_new_file_is_always_allowed() {
    // A create races with nobody, and requiring a prior read would make it
    // impossible to write a file that does not exist yet.
    let (_d, path) = temp_file("x");
    assert!(!is_stale_write(Uuid::new_v4(), &path, None));
}

#[test]
fn writing_what_this_session_read_is_allowed() {
    let session = Uuid::new_v4();
    let (_d, path) = temp_file("original");
    record(session, &path, "original");
    assert!(!is_stale_write(session, &path, Some("original")));
    forget_session(session);
}

#[test]
fn a_file_changed_since_this_session_read_it_is_refused() {
    // The reported failure: agent A reads, agent B writes, agent A writes and
    // B's change vanishes.
    let session = Uuid::new_v4();
    let (_d, path) = temp_file("original");
    record(session, &path, "original");

    assert!(
        is_stale_write(session, &path, Some("changed by another agent")),
        "a whole-file write over someone else's change must be refused"
    );
    forget_session(session);
}

#[test]
fn re_reading_clears_the_refusal() {
    // The refusal has to be recoverable or it is just a wall. Reading again
    // records the newer content and the write proceeds.
    let session = Uuid::new_v4();
    let (_d, path) = temp_file("original");
    record(session, &path, "original");
    assert!(is_stale_write(session, &path, Some("theirs")));

    record(session, &path, "theirs"); // the agent re-reads
    assert!(
        !is_stale_write(session, &path, Some("theirs")),
        "after re-reading, the write must go through"
    );
    forget_session(session);
}

#[test]
fn a_session_that_never_read_the_file_is_allowed_through() {
    // Deliberately NOT refused. `write_file` is documented as create-or-replace
    // and is legitimately used to generate a file outright; refusing here would
    // break the tool's contract to guard a case nobody reported. An agent
    // rewriting existing source reads it first, and that read is what arms the
    // guard.
    let (_d, path) = temp_file("someone elses work");
    assert!(!is_stale_write(
        Uuid::new_v4(),
        &path,
        Some("someone elses work")
    ));
}

#[test]
fn writing_twice_in_a_row_is_allowed() {
    // The writer's own output must not look like another agent's change on the
    // next write, or a single agent could not edit a file twice.
    let session = Uuid::new_v4();
    let (_d, path) = temp_file("v1");
    record(session, &path, "v1");
    assert!(!is_stale_write(session, &path, Some("v1")));

    record(session, &path, "v2"); // what write_file records after writing
    assert!(!is_stale_write(session, &path, Some("v2")));
    forget_session(session);
}

#[test]
fn sessions_do_not_share_what_they_have_seen() {
    // Two agents in one directory is the whole scenario: one reading a file
    // must not license the other to overwrite it.
    let reader = Uuid::new_v4();
    let other = Uuid::new_v4();
    let (_d, path) = temp_file("shared");
    record(reader, &path, "shared");

    assert!(!is_stale_write(reader, &path, Some("shared")));

    // The reader's record must not arm the guard for the other session, and
    // must not disarm it either: what matters is that `other` is judged on its
    // own history, which is empty.
    assert!(!is_stale_write(other, &path, Some("shared")));

    // Now the file moves under the reader. Only the reader is stale — the
    // other session, which never read it, is unaffected.
    assert!(is_stale_write(reader, &path, Some("changed")));
    assert!(!is_stale_write(other, &path, Some("changed")));
    forget_session(reader);
}

#[test]
fn forgetting_a_session_leaves_other_sessions_intact() {
    // Sub-agent sessions are created per spawn and never revisited, so they are
    // dropped — but dropping one must not disarm the guard for everyone else.
    let keep = Uuid::new_v4();
    let drop = Uuid::new_v4();
    let (_d, path) = temp_file("shared");
    record(keep, &path, "shared");
    record(drop, &path, "shared");

    forget_session(drop);

    // The kept session is still guarded: a change under it is still caught.
    assert!(
        is_stale_write(keep, &path, Some("changed")),
        "forgetting one session must not disarm another"
    );
    // The dropped one has no history, so it is judged as never having read it.
    assert!(!is_stale_write(drop, &path, Some("changed")));
    forget_session(keep);
}
