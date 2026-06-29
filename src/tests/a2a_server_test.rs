use super::*;
use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

async fn test_state() -> A2aState {
    use crate::a2a::test_helpers::helpers;

    A2aState {
        task_store: handler::new_task_store(),
        cancel_store: handler::new_cancel_store(),
        host: "127.0.0.1".to_string(),
        port: 18790,
        agent_service: helpers::placeholder_agent_service().await,
        service_context: helpers::placeholder_service_context().await,
        api_key: None,
    }
}

#[tokio::test]
async fn test_health_endpoint() {
    let app = build_router(test_state().await, &[]);
    let req = Request::builder()
        .uri("/a2a/health")
        .body(Body::empty())
        .expect("request");

    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_agent_card_endpoint() {
    let app = build_router(test_state().await, &[]);
    let req = Request::builder()
        .uri("/.well-known/agent.json")
        .body(Body::empty())
        .expect("request");

    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
}
