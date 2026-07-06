//! Slack file upload via the supported external flow (#370 follow-up).
//!
//! Slack removed the legacy `files.upload` API; every upload must go
//! through getUploadURLExternal -> POST bytes -> completeUploadExternal.
//! One helper so the handler's image and TTS uploads and any future
//! caller share the same three-step implementation and error reporting.

use slack_morphism::prelude::*;

/// Upload `bytes` as `filename` into `channel`. Errors are returned as a
/// single human-readable string; callers decide how loudly to log.
pub(crate) async fn upload_external<'a>(
    session: &SlackClientSession<'a, slack_morphism::hyper_tokio::SlackClientHyperHttpsConnector>,
    channel: SlackChannelId,
    bytes: Vec<u8>,
    filename: &str,
    content_type: &str,
    thread_ts: Option<SlackTs>,
) -> Result<(), String> {
    let ticket = session
        .get_upload_url_external(&SlackApiFilesGetUploadUrlExternalRequest {
            filename: filename.to_string(),
            length: bytes.len(),
            alt_txt: None,
            snippet_type: None,
        })
        .await
        .map_err(|e| format!("get upload URL: {e}"))?;
    session
        .files_upload_via_url(&SlackApiFilesUploadViaUrlRequest {
            upload_url: ticket.upload_url,
            content: bytes,
            content_type: content_type.to_string(),
        })
        .await
        .map_err(|e| format!("upload bytes: {e}"))?;
    session
        .files_complete_upload_external(&SlackApiFilesCompleteUploadExternalRequest {
            files: vec![SlackApiFilesComplete {
                id: ticket.file_id,
                title: Some(filename.to_string()),
            }],
            channel_id: Some(channel),
            initial_comment: None,
            thread_ts,
        })
        .await
        .map_err(|e| format!("complete upload: {e}"))?;
    Ok(())
}
