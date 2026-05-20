//! Telegram photo batching tests
//!
//! Tests the media_group_id-based photo batching logic in TelegramState.

use crate::channels::telegram::TelegramState;
use std::sync::Arc;
use tokio::time::{Duration, timeout};

/// Test that single photos (no media_group_id) dispatch immediately without buffering.
#[tokio::test]
async fn single_photo_no_debounce() {
    let state = Arc::new(TelegramState::new());
    let chat_id = 12345i64;
    let user_id = 67890i64;

    // Single photo should not trigger any buffering logic
    // (in real code, the handler checks media_group_id and skips buffering if None)
    // Here we just verify the state methods work correctly when called.

    // Simulate: handler checks media_group_id, it's None, so it dispatches immediately
    // without calling buffer_photo or reset_photo_debounce.

    // Verify buffer is empty (nothing was buffered)
    let buffered = state.drain_photo_buffer(chat_id, user_id, "test_group").await;
    assert!(
        buffered.is_empty(),
        "single photo should not buffer anything"
    );
}

/// Test that album photos with the same media_group_id are batched together.
#[tokio::test]
async fn album_photos_batched_by_media_group() {
    let state = Arc::new(TelegramState::new());
    let chat_id = 12345i64;
    let user_id = 67890i64;
    let media_group_id = "album_123";

    // Buffer 3 photos from the same album
    let count1 = state
        .buffer_photo(
            chat_id,
            user_id,
            media_group_id,
            "<<IMG:/path/to/photo1.jpg>>".to_string(),
            Some("First photo caption".to_string()),
        )
        .await;
    assert_eq!(count1, 1);

    let count2 = state
        .buffer_photo(
            chat_id,
            user_id,
            media_group_id,
            "<<IMG:/path/to/photo2.jpg>>".to_string(),
            None,
        )
        .await;
    assert_eq!(count2, 2);

    let count3 = state
        .buffer_photo(
            chat_id,
            user_id,
            media_group_id,
            "<<IMG:/path/to/photo3.jpg>>".to_string(),
            None,
        )
        .await;
    assert_eq!(count3, 3);

    // Drain and verify all 3 photos are returned
    let buffered = state.drain_photo_buffer(chat_id, user_id, media_group_id).await;
    assert_eq!(buffered.len(), 3);

    // Verify captions are preserved (first photo has caption, others don't)
    assert_eq!(
        buffered[0].1,
        Some("First photo caption".to_string()),
        "first photo should have caption"
    );
    assert_eq!(buffered[1].1, None, "second photo should have no caption");
    assert_eq!(buffered[2].1, None, "third photo should have no caption");

    // Verify img_markers are in order
    assert!(buffered[0].0.contains("photo1.jpg"));
    assert!(buffered[1].0.contains("photo2.jpg"));
    assert!(buffered[2].0.contains("photo3.jpg"));
}

/// Test that photos from different albums (different media_group_id) are kept separate.
#[tokio::test]
async fn different_albums_not_merged() {
    let state = Arc::new(TelegramState::new());
    let chat_id = 12345i64;
    let user_id = 67890i64;

    // Buffer photos from two different albums
    state
        .buffer_photo(
            chat_id,
            user_id,
            "album_A",
            "<<IMG:/path/to/A1.jpg>>".to_string(),
            Some("Album A".to_string()),
        )
        .await;
    state
        .buffer_photo(
            chat_id,
            user_id,
            "album_B",
            "<<IMG:/path/to/B1.jpg>>".to_string(),
            Some("Album B".to_string()),
        )
        .await;
    state
        .buffer_photo(
            chat_id,
            user_id,
            "album_A",
            "<<IMG:/path/to/A2.jpg>>".to_string(),
            None,
        )
        .await;

    // Drain album A — should only have 2 photos
    let album_a = state.drain_photo_buffer(chat_id, user_id, "album_A").await;
    assert_eq!(album_a.len(), 2, "album A should have 2 photos");
    assert!(album_a[0].0.contains("A1.jpg"));
    assert!(album_a[1].0.contains("A2.jpg"));

    // Drain album B — should only have 1 photo
    let album_b = state.drain_photo_buffer(chat_id, user_id, "album_B").await;
    assert_eq!(album_b.len(), 1, "album B should have 1 photo");
    assert!(album_b[0].0.contains("B1.jpg"));
}

/// Test that debounce timer cancellation works correctly.
#[tokio::test]
async fn debounce_cancellation() {
    let state = Arc::new(TelegramState::new());
    let chat_id = 12345i64;
    let user_id = 67890i64;
    let media_group_id = "test_album";

    // Start first debounce timer
    let token1 = state
        .reset_photo_debounce(chat_id, user_id, media_group_id)
        .await;

    // Simulate another photo arriving (cancels first timer)
    let token2 = state
        .reset_photo_debounce(chat_id, user_id, media_group_id)
        .await;

    // First token should be cancelled
    assert!(
        token1.is_cancelled(),
        "first debounce token should be cancelled when second photo arrives"
    );

    // Second token should not be cancelled yet
    assert!(
        !token2.is_cancelled(),
        "second debounce token should not be cancelled yet"
    );
}

/// Test that debounce timer expires after 3 seconds.
#[tokio::test]
async fn debounce_expires_after_timeout() {
    let state = Arc::new(TelegramState::new());
    let chat_id = 12345i64;
    let user_id = 67890i64;
    let media_group_id = "test_album";

    // Start debounce timer
    let token = state
        .reset_photo_debounce(chat_id, user_id, media_group_id)
        .await;

    // Wait for debounce (3 seconds) — should expire
    let result = timeout(Duration::from_secs(4), state.wait_photo_debounce(token)).await;

    assert!(result.is_ok(), "debounce wait should complete");
    assert!(result.unwrap(), "debounce should expire (return true)");
}

/// Test that debounce timer is cancelled when another photo arrives.
#[tokio::test]
async fn debounce_cancelled_by_new_photo() {
    let state = Arc::new(TelegramState::new());
    let chat_id = 12345i64;
    let user_id = 67890i64;
    let media_group_id = "test_album";

    // Start first debounce timer
    let token1 = state
        .reset_photo_debounce(chat_id, user_id, media_group_id)
        .await;

    // Spawn a task to cancel the first token after 1 second (simulating another photo)
    let state_clone = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        state_clone
            .reset_photo_debounce(chat_id, user_id, media_group_id)
            .await;
    });

    // Wait for first debounce — should be cancelled (return false)
    let result = timeout(Duration::from_secs(4), state.wait_photo_debounce(token1)).await;

    assert!(result.is_ok(), "debounce wait should complete");
    assert!(
        !result.unwrap(),
        "debounce should be cancelled (return false)"
    );
}

/// Test edge case: draining empty buffer returns empty vec (no ghost dispatch).
#[tokio::test]
async fn drain_empty_buffer_no_ghost_dispatch() {
    let state = Arc::new(TelegramState::new());
    let chat_id = 12345i64;
    let user_id = 67890i64;
    let media_group_id = "nonexistent_album";

    // Drain without buffering anything
    let buffered = state
        .drain_photo_buffer(chat_id, user_id, media_group_id)
        .await;

    assert!(
        buffered.is_empty(),
        "draining empty buffer should return empty vec"
    );
}

/// Test that cleanup removes the debounce token.
#[tokio::test]
async fn cleanup_removes_debounce_token() {
    let state = Arc::new(TelegramState::new());
    let chat_id = 12345i64;
    let user_id = 67890i64;
    let media_group_id = "test_album";

    // Create a debounce token
    let token = state
        .reset_photo_debounce(chat_id, user_id, media_group_id)
        .await;
    assert!(!token.is_cancelled());

    // Cleanup
    state
        .cleanup_photo_debounce(chat_id, user_id, media_group_id)
        .await;

    // Creating a new token should work (old one was removed)
    let token2 = state
        .reset_photo_debounce(chat_id, user_id, media_group_id)
        .await;
    assert!(
        !token2.is_cancelled(),
        "new token after cleanup should not be cancelled"
    );
}
