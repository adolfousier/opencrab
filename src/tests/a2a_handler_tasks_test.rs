use crate::a2a::handler::tasks::*;
use crate::a2a::handler::{new_cancel_store, new_task_store};
use crate::a2a::types::error_codes;

#[tokio::test]
async fn test_cancel_task_not_found() {
    use crate::a2a::test_helpers::helpers;

    let store = new_task_store();
    let cancel_store = new_cancel_store();
    let ctx = helpers::placeholder_service_context().await;
    let resp = handle_cancel_task(
        serde_json::json!(1),
        serde_json::json!({"id": "nonexistent"}),
        store,
        cancel_store,
        &ctx.pool(),
    )
    .await;
    assert!(resp.error.is_some());
    assert_eq!(
        resp.error.as_ref().expect("err").code,
        error_codes::TASK_NOT_FOUND
    );
}
