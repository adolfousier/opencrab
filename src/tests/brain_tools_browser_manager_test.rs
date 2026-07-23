use crate::brain::tools::browser::manager::*;
use tokio;

#[test]
fn test_manager_new() {
    let mgr = BrowserManager::new(Default::default());
    let _ = mgr.clone();
}

#[test]
fn test_manager_with_headless() {
    let mgr = BrowserManager::with_headless(false, Default::default());
    let _ = mgr.clone();
}

#[tokio::test]
async fn test_is_headless_default() {
    let mgr = BrowserManager::with_headless(true, Default::default());
    assert!(mgr.is_headless().await);
}

#[tokio::test]
async fn test_is_headless_false() {
    let mgr = BrowserManager::with_headless(false, Default::default());
    assert!(!mgr.is_headless().await);
}

#[tokio::test]
async fn test_set_headless_no_change() {
    let mgr = BrowserManager::with_headless(true, Default::default());
    // Already headless — no change
    assert!(!mgr.set_headless(true).await);
}

#[tokio::test]
async fn test_set_headless_switch() {
    let mgr = BrowserManager::with_headless(true, Default::default());
    assert!(mgr.is_headless().await);

    if BrowserManager::has_display() {
        // Has display — switching to headed should succeed
        assert!(mgr.set_headless(false).await);
        assert!(!mgr.is_headless().await);
        // Switch back
        assert!(mgr.set_headless(true).await);
        assert!(mgr.is_headless().await);
    } else {
        // No display — switching to headed should be rejected, stays headless
        assert!(!mgr.set_headless(false).await);
        assert!(mgr.is_headless().await);
    }
}

#[tokio::test]
async fn test_list_pages_empty() {
    let mgr = BrowserManager::new(Default::default());
    assert!(mgr.list_pages().await.is_empty());
}

#[tokio::test]
async fn test_close_nonexistent() {
    use crate::brain::tools::browser::CloseOutcome;
    let mgr = BrowserManager::new(Default::default());
    assert_eq!(
        mgr.close_page("nonexistent").await,
        CloseOutcome::NothingOpen
    );
}

#[test]
fn test_detect_browser_finds_something() {
    // On dev machines there should be at least one Chromium browser
    let result = detect_browser();
    if let Some(info) = result {
        assert!(!info.name.is_empty());
        assert!(info.path.exists());
        tracing::info!("Detected: {} at {}", info.name, info.path.display());
    }
    // On CI with no browser installed, None is acceptable
}

#[test]
fn test_known_browsers_not_empty() {
    let browsers = known_browsers();
    assert!(browsers.len() >= 7); // Chrome, Brave, Edge, Arc, Vivaldi, Opera, Chromium
}

#[test]
fn test_is_profile_locked_nonexistent() {
    let dir = std::path::PathBuf::from("/tmp/nonexistent-browser-profile-test");
    assert!(!is_profile_locked(&dir));
}

#[test]
fn test_detect_default_browser_id() {
    // Just ensure it doesn't panic; the result depends on system config.
    let _ = detect_default_browser_id();
}
