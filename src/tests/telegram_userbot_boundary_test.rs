//! Compile-time feature boundary audit for the receive-only userbot.

#[test]
fn userbot_source_has_no_mutation_or_agent_dispatch_surface() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/channels/telegram/userbot");
    let mut source = String::new();
    for entry in std::fs::read_dir(root).expect("userbot dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            source.push_str(&std::fs::read_to_string(path).expect("source"));
        }
    }

    for forbidden in [
        "send_reactions(",
        "send_message(",
        "edit_message(",
        "delete_messages(",
        "download_media(",
        "search_all_messages(",
        "handle_message(",
        "AgentService",
        "userbot/tools",
        "allow_dangerous",
        "outbound_allowlist",
        "chat_permissions",
        "wait_reply",
        "schedule_unix",
    ] {
        assert!(
            !source.contains(forbidden),
            "receive-only boundary contains forbidden surface: {forbidden}"
        );
    }
}

#[test]
fn watch_gate_precedes_conversion_and_storage() {
    let watch = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/channels/telegram/userbot/watch.rs"),
    )
    .expect("watch source");
    let gate = watch.find("chat_allowed(&allowed").expect("allowlist gate");
    let convert = watch
        .find("to_channel_message(&message)")
        .expect("conversion");
    let store = watch.find("messages.insert(&row)").expect("storage");
    assert!(gate < convert && convert < store);
}
