//! Regression tests for the rich API client parameterised `api_url` (#1088).
//!
//! Verifies that the4 public functions in `crate::channels::telegram::rich::api`
//! route through a caller-supplied base URL instead of hardcoding
//! `api.telegram.org`. Uses `mockito` to intercept the HTTP call and confirm
//! the constructed endpoint is hit.

use crate::channels::telegram::rich::api;

#[tokio::test]
async fn send_rich_markdown_id_uses_custom_api_url() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/botTESTTOKEN/sendRichMessage")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"result":{"message_id":42}}"#)
        .create_async()
        .await;

    let result = api::send_rich_markdown_id(
        &server.url(),
        "TESTTOKEN",
        12345,
        None,
        "hello **world**",
        "test",
        "-",
    )
    .await;

    assert!(result.is_ok(), "send should succeed: {:?}", result.err());
    assert_eq!(result.unwrap(), 42);
    mock.assert_async().await;
}

#[tokio::test]
async fn send_rich_html_id_uses_custom_api_url() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/botTESTTOKEN/sendRichMessage")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"result":{"message_id":99}}"#)
        .create_async()
        .await;

    let result = api::send_rich_html_id(
        &server.url(),
        "TESTTOKEN",
        67890,
        None,
        "<b>bold</b>",
        None,
        "test",
        "-",
    )
    .await;

    assert!(result.is_ok(), "send should succeed: {:?}", result.err());
    assert_eq!(result.unwrap(), 99);
    mock.assert_async().await;
}

#[tokio::test]
async fn edit_rich_html_uses_custom_api_url() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/botTESTTOKEN/editMessageText")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"result":true}"#)
        .create_async()
        .await;

    let result = api::edit_rich_html(
        &server.url(),
        "TESTTOKEN",
        12345,
        1,
        "<b>edited</b>",
        None,
        "test",
        "-",
    )
    .await;

    assert!(result.is_ok(), "edit should succeed: {:?}", result.err());
    mock.assert_async().await;
}

#[tokio::test]
async fn send_rich_markdown_media_id_uses_custom_api_url() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/botTESTTOKEN/sendRichMessage")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"result":{"message_id":77}}"#)
        .create_async()
        .await;

    let result = api::send_rich_markdown_media_id(
        &server.url(),
        "TESTTOKEN",
        11111,
        None,
        "![img](tg://photo?id=1)",
        &[crate::channels::telegram::rich::mermaid::MediaEntry {
            id: "1".to_string(),
            url: "https://example.com/img.png".to_string(),
        }],
        "test",
        "-",
    )
    .await;

    assert!(result.is_ok(), "media send should succeed: {:?}", result.err());
    assert_eq!(result.unwrap(), 77);
    mock.assert_async().await;
}
