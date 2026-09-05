//! Session-scoped fully-read path tracking (#1168).

use crate::brain::tools::read_state::*;
use uuid::Uuid;

#[test]
fn tracks_per_session_and_path() {
    let sid = Uuid::new_v4();
    let other_sid = Uuid::new_v4();
    let dir = std::env::temp_dir().join(format!("read_state_{}", sid.simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("f.txt");
    std::fs::write(&file, "x").unwrap();

    assert!(!was_fully_read(sid, &file));
    mark_fully_read(sid, &file);
    assert!(was_fully_read(sid, &file));

    // Another session has not read it.
    assert!(!was_fully_read(other_sid, &file));
    // Relative spelling of the same file resolves to the same key.
    let rel = dir.join("./f.txt");
    assert!(was_fully_read(sid, &rel));

    std::fs::remove_dir_all(&dir).ok();
}
