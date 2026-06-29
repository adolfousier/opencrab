use super::*;
use std::path::PathBuf;

#[test]
fn test_new() {
    let updater = SelfUpdater::new(
        PathBuf::from("/tmp/project"),
        PathBuf::from("/tmp/project/target/release/opencrabs"),
    );
    assert_eq!(updater.project_root(), std::path::Path::new("/tmp/project"));
    assert_eq!(
        updater.binary_path(),
        std::path::Path::new("/tmp/project/target/release/opencrabs")
    );
}
