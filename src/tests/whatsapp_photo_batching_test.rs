//! Tests for WhatsApp photo batching logic.
//!
//! Verifies that multiple photos arriving in quick succession are batched
//! together and dispatched as a single message, preventing the cancellation
//! issue where only the last photo would be processed.

#[cfg(test)]
mod tests {
    use crate::channels::whatsapp::WhatsAppState;

    #[tokio::test]
    async fn test_buffer_single_photo() {
        let state = WhatsAppState::new();
        let phone = "+1234567890".to_string();
        
        let count = state.buffer_photo(&phone, "<<IMG:/tmp/img1.jpg>>".to_string(), None).await;
        assert_eq!(count, 1);
        
        let (markers, caption) = state.drain_photo_buffer(&phone).await;
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], "<<IMG:/tmp/img1.jpg>>");
        assert!(caption.is_none());
    }

    #[tokio::test]
    async fn test_buffer_multiple_photos() {
        let state = WhatsAppState::new();
        let phone = "+1234567890".to_string();
        
        state.buffer_photo(&phone, "<<IMG:/tmp/img1.jpg>>".to_string(), Some("First caption".to_string())).await;
        state.buffer_photo(&phone, "<<IMG:/tmp/img2.jpg>>".to_string(), None).await;
        state.buffer_photo(&phone, "<<IMG:/tmp/img3.jpg>>".to_string(), None).await;
        
        let (markers, caption) = state.drain_photo_buffer(&phone).await;
        assert_eq!(markers.len(), 3);
        assert_eq!(markers[0], "<<IMG:/tmp/img1.jpg>>");
        assert_eq!(markers[1], "<<IMG:/tmp/img2.jpg>>");
        assert_eq!(markers[2], "<<IMG:/tmp/img3.jpg>>");
        // Should use first non-empty caption
        assert_eq!(caption, Some("First caption".to_string()));
    }

    #[tokio::test]
    async fn test_drain_empty_buffer() {
        let state = WhatsAppState::new();
        let phone = "+1234567890".to_string();
        
        let (markers, caption) = state.drain_photo_buffer(&phone).await;
        assert!(markers.is_empty());
        assert!(caption.is_none());
    }

    #[tokio::test]
    async fn test_drain_clears_buffer() {
        let state = WhatsAppState::new();
        let phone = "+1234567890".to_string();
        
        state.buffer_photo(&phone, "<<IMG:/tmp/img1.jpg>>".to_string(), None).await;
        let (markers1, _) = state.drain_photo_buffer(&phone).await;
        assert_eq!(markers1.len(), 1);
        
        // Second drain should be empty
        let (markers2, _) = state.drain_photo_buffer(&phone).await;
        assert!(markers2.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_phones_independent_buffers() {
        let state = WhatsAppState::new();
        let phone1 = "+1111111111".to_string();
        let phone2 = "+2222222222".to_string();
        
        state.buffer_photo(&phone1, "<<IMG:/tmp/img1.jpg>>".to_string(), None).await;
        state.buffer_photo(&phone2, "<<IMG:/tmp/img2.jpg>>".to_string(), None).await;
        state.buffer_photo(&phone2, "<<IMG:/tmp/img3.jpg>>".to_string(), None).await;
        
        let (markers1, _) = state.drain_photo_buffer(&phone1).await;
        let (markers2, _) = state.drain_photo_buffer(&phone2).await;
        
        assert_eq!(markers1.len(), 1);
        assert_eq!(markers2.len(), 2);
    }

    #[tokio::test]
    async fn test_caption_selection_first_non_empty() {
        let state = WhatsAppState::new();
        let phone = "+1234567890".to_string();
        
        state.buffer_photo(&phone, "<<IMG:/tmp/img1.jpg>>".to_string(), None).await;
        state.buffer_photo(&phone, "<<IMG:/tmp/img2.jpg>>".to_string(), Some("".to_string())).await;
        state.buffer_photo(&phone, "<<IMG:/tmp/img3.jpg>>".to_string(), Some("Actual caption".to_string())).await;
        
        let (_, caption) = state.drain_photo_buffer(&phone).await;
        // Should find first non-empty caption
        assert_eq!(caption, Some("Actual caption".to_string()));
    }

    #[tokio::test]
    async fn test_debounce_reset_cancels_previous() {
        let state = WhatsAppState::new();
        let phone = "+1234567890".to_string();
        
        let token1 = state.reset_photo_debounce(&phone).await;
        let token2 = state.reset_photo_debounce(&phone).await;
        
        // First token should be cancelled
        assert!(token1.is_cancelled());
        // Second token should still be active
        assert!(!token2.is_cancelled());
    }

    #[tokio::test]
    async fn test_wait_photo_debounce_cancelled() {
        let state = WhatsAppState::new();
        let phone = "+1234567890".to_string();
        
        let token1 = state.reset_photo_debounce(&phone).await;
        
        // Cancel it immediately
        token1.cancel();
        
        // Should return false (cancelled)
        let result = state.wait_photo_debounce(&token1).await;
        assert!(!result);
    }

    #[tokio::test]
    async fn test_wait_photo_debounce_expired() {
        let state = WhatsAppState::new();
        let phone = "+1234567890".to_string();
        
        let token = state.reset_photo_debounce(&phone).await;
        
        // Wait with a short timeout to test expiration
        tokio::select! {
            result = state.wait_photo_debounce(&token) => {
                // Should return true (expired)
                assert!(result);
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(4)) => {
                panic!("Debounce did not expire within expected time");
            }
        }
    }

    #[tokio::test]
    async fn test_cleanup_photo_debounce() {
        let state = WhatsAppState::new();
        let phone = "+1234567890".to_string();
        
        state.reset_photo_debounce(&phone).await;
        state.cleanup_photo_debounce(&phone).await;
        
        // Cleanup should remove the token
        let debounce = state.photo_debounce.lock().await;
        assert!(!debounce.contains_key(&phone));
    }

    #[tokio::test]
    async fn test_rapid_photo_upload_simulation() {
        use std::sync::Arc;
        let state = Arc::new(WhatsAppState::new());
        let phone = "+1234567890".to_string();
        
        // Simulate 5 photos arriving rapidly
        let mut handles = vec![];
        for i in 1..=5 {
            let state = state.clone();
            let phone = phone.clone();
            let handle = tokio::spawn(async move {
                let marker = format!("<<IMG:/tmp/img{}.jpg>>", i);
                let caption = if i == 1 { Some("Check these out".to_string()) } else { None };
                state.buffer_photo(&phone, marker, caption).await;
                let token = state.reset_photo_debounce(&phone).await;
                (i, token)
            });
            handles.push(handle);
            
            // Small delay to simulate network
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        
        // Collect all tokens
        let mut tokens = vec![];
        for handle in handles {
            let (i, token) = handle.await.unwrap();
            tokens.push((i, token));
        }
        
        // All but the last token should be cancelled
        for (i, token) in &tokens[..4] {
            assert!(token.is_cancelled(), "Token {} should be cancelled", i);
        }
        assert!(!tokens[4].1.is_cancelled(), "Last token should not be cancelled");
        
        // Wait for the last token to expire
        let result = state.wait_photo_debounce(&tokens[4].1).await;
        assert!(result, "Last token should expire");
        
        // Drain should get all 5 photos
        let (markers, caption) = state.drain_photo_buffer(&phone).await;
        assert_eq!(markers.len(), 5);
        assert_eq!(caption, Some("Check these out".to_string()));
    }
}
