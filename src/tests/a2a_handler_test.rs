use crate::a2a::handler::*;
use crate::a2a::types::JsonRpcRequest;
// tasks/get and tasks/cancel tests don't need AgentService
#[tokio::test]
async fn test_get_task_not_found() {
    let store = new_task_store();
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "tasks/get".to_string(),
        params: serde_json::json!({"id": "nonexistent"}),
        id: serde_json::json!(2),
    };
    let resp = tasks::handle_get_task(req.id, req.params, store).await;
    assert!(resp.error.is_some());
    assert_eq!(resp.error.as_ref().expect("err").code, -32001);
}

#[tokio::test]
async fn test_unknown_method() {
    use crate::a2a::test_helpers::helpers;

    let store = new_task_store();
    let cancel_store = new_cancel_store();
    let agent = helpers::placeholder_agent_service().await;
    let ctx = helpers::placeholder_service_context().await;
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "unknown/method".to_string(),
        params: serde_json::json!({}),
        id: serde_json::json!(99),
    };
    let resp = dispatch(req, store, cancel_store, agent, ctx).await;
    assert!(resp.error.is_some());
    assert_eq!(resp.error.as_ref().expect("err").code, -32601);
}
