//! Photo batching: WhatsApp sends each image of an album as a separate
//! message, so markers are buffered per chat and dispatched together once a
//! short debounce expires without another photo arriving.

use tokio_util::sync::CancellationToken;

use super::WhatsAppState;

/// How long a chat's photo buffer waits for another image before dispatch.
const PHOTO_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(3);

impl WhatsAppState {
    /// Buffer a photo marker for batching. Returns the current buffer size.
    pub async fn buffer_photo(
        &self,
        chat_jid: &str,
        img_marker: String,
        caption: Option<String>,
    ) -> usize {
        let mut buffer = self.photo_buffer.lock().await;
        let entry = buffer.entry(chat_jid.to_string()).or_default();
        entry.push((img_marker, caption));
        entry.len()
    }

    /// Drain all buffered photos for a chat. Returns the markers and the
    /// first non-empty caption found (WhatsApp only captions the first image).
    pub async fn drain_photo_buffer(&self, chat_jid: &str) -> (Vec<String>, Option<String>) {
        let mut buffer = self.photo_buffer.lock().await;
        if let Some(entries) = buffer.remove(chat_jid) {
            let caption = entries
                .iter()
                .find_map(|(_, c)| c.as_ref().filter(|s| !s.trim().is_empty()).cloned());
            let markers: Vec<String> = entries.into_iter().map(|(m, _)| m).collect();
            (markers, caption)
        } else {
            (Vec::new(), None)
        }
    }

    /// Reset the photo debounce timer for a chat. Returns a new CancellationToken
    /// that will be cancelled if another photo arrives before it expires.
    pub async fn reset_photo_debounce(&self, chat_jid: &str) -> CancellationToken {
        let mut debounce = self.photo_debounce.lock().await;
        if let Some(old_token) = debounce.remove(chat_jid) {
            old_token.cancel();
        }
        let token = CancellationToken::new();
        debounce.insert(chat_jid.to_string(), token.clone());
        token
    }

    /// Wait for the photo debounce to expire. Returns true if the timer expired
    /// (this task should process the buffer), false if cancelled (another photo
    /// arrived and will handle it).
    pub async fn wait_photo_debounce(&self, token: &CancellationToken) -> bool {
        tokio::select! {
            _ = token.cancelled() => false,
            _ = tokio::time::sleep(PHOTO_DEBOUNCE) => true,
        }
    }

    /// Clean up the debounce token after processing.
    pub async fn cleanup_photo_debounce(&self, chat_jid: &str) {
        let mut debounce = self.photo_debounce.lock().await;
        debounce.remove(chat_jid);
    }
}
