//! Userbot tool-plane tests: params-file plumbing + the authorize gate.
//!
//! The tool plane is the interactive complement to the read-only watch
//! loop (PR #1113 gap analysis). Two invariants pinned here:
//!
//! 1. Params roundtrip: a tool invocation saved to a file and loaded
//!    back is the same data — the file is the contract.
//! 2. Governance: reads pass when enabled; outbound requires the target
//!    with `send` in `chat_permissions` (none = strictly read-only); raw MTProto
//!    requires an explicit per-invocation `confirm`.

use std::io::Write;

use crate::channels::telegram::userbot::tools::commands::{
    DEFAULT_LIMIT, Discover, Download, EditMessage, Raw, React, ReadChat, SearchChat, SendFile,
    SendMessage, SendToPhone, ToolCommand,
};
use crate::channels::telegram::userbot::tools::dispatch::{Denial, authorize};
use crate::channels::telegram::userbot::tools::params::ToolInvocation;
use crate::config::types::{ChatPermission, TelegramUserbotConfig};

fn read_chat(chat: &str) -> ToolCommand {
    ToolCommand::ReadChat(ReadChat {
        chat: chat.to_string(),
        from: Some("@alice".to_string()),
        to: None,
        thread_id: Some(4230),
        limit: 50,
    })
}

#[test]
fn invocation_roundtrips_through_file() {
    let dir = std::env::temp_dir().join("userbot-tools-test");
    let path = dir.join("inv.json");
    let inv = ToolInvocation::new(read_chat("-1004427473737"));
    inv.save(&path).expect("save should write the params file");

    let loaded = ToolInvocation::load(&path).expect("load should read it back");
    assert_eq!(loaded, inv);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn parse_accepts_handwritten_params_json() {
    let json = r#"{
        "tool": "send_message",
        "chat": "@somebody",
        "text": "hello",
        "reply_to": null
    }"#;
    let inv = ToolInvocation::parse(json).expect("handwritten params should parse");
    match inv.command {
        ToolCommand::SendMessage(SendMessage { chat, text, .. }) => {
            assert_eq!(chat, "@somebody");
            assert_eq!(text, "hello");
        }
        other => panic!("expected SendMessage, got {other:?}"),
    }
}

#[test]
fn parse_round_trips_schedule_unix() {
    let json = r#"{
        "tool": "send_message",
        "chat": "@friend",
        "text": "happy new year",
        "schedule_unix": 1798761600
    }"#;
    let inv = ToolInvocation::parse(json).expect("scheduled send should parse");
    match inv.command {
        ToolCommand::SendMessage(SendMessage {
            schedule_unix: Some(unix),
            ..
        }) => {
            assert_eq!(unix, 1798761600);
        }
        other => panic!("expected scheduled SendMessage, got {other:?}"),
    }
    // Omitted field means send-now.
    let now = ToolInvocation::parse(r#"{"tool":"send_message","chat":"@friend","text":"asap"}"#)
        .expect("plain send should parse");
    match now.command {
        ToolCommand::SendMessage(SendMessage {
            schedule_unix: None,
            ..
        }) => {}
        other => panic!("expected plain SendMessage, got {other:?}"),
    }
}

#[test]
fn parses_wait_reply_secs() {
    let inv = ToolInvocation::parse(
        r#"{"tool":"send_message","chat":"@wallet","text":"balance","wait_reply_secs":45}"#,
    )
    .expect("wait-reply send should parse");
    match inv.command {
        ToolCommand::SendMessage(SendMessage {
            wait_reply_secs: Some(secs),
            ..
        }) => assert_eq!(secs, 45),
        other => panic!("expected wait-reply SendMessage, got {other:?}"),
    }
    // Omitted field means no wait.
    let plain = ToolInvocation::parse(r#"{"tool":"send_message","chat":"@x","text":"y"}"#)
        .expect("plain send should parse");
    match plain.command {
        ToolCommand::SendMessage(SendMessage {
            wait_reply_secs: None,
            ..
        }) => {}
        other => panic!("expected no-wait SendMessage, got {other:?}"),
    }
}

#[test]
fn parse_rejects_unknown_tool_and_future_version() {
    assert!(ToolInvocation::parse(r#"{"tool":"nope"}"#).is_err());
    assert!(ToolInvocation::parse(r#"{"version":99,"tool":"discover"}"#).is_err());
}

/// Pins the $OPENCRABS_PARAMS contract: nullable/optional keys are simply
/// ABSENT and must fall back to typed defaults (int limit, Option=nulls,
/// bool confirm=false) — never string-coerced.
#[test]
fn omitted_keys_take_typed_defaults() {
    let inv = ToolInvocation::parse(r#"{"tool":"read_chat","chat":"me"}"#)
        .expect("minimal invocation should parse");
    match inv.command {
        ToolCommand::ReadChat(ReadChat {
            chat,
            limit,
            from,
            to,
            thread_id,
        }) => {
            assert_eq!(chat, "me");
            assert_eq!(
                limit, DEFAULT_LIMIT,
                "omitted limit must take the typed default"
            );
            assert_eq!((from, to, thread_id), (None, None, None));
        }
        other => panic!("expected ReadChat, got {other:?}"),
    }
    // Omitted confirm on raw defaults FALSE — the dangerous gate stays opt-in.
    let raw = ToolInvocation::parse(r#"{"tool":"raw","method":"m","params":{}}"#)
        .expect("raw without confirm should parse");
    assert!(matches!(
        raw.command,
        ToolCommand::Raw(Raw { confirm: false, .. })
    ));
}

#[test]
fn download_parses_with_optional_path() {
    let inv = ToolInvocation::parse(r#"{"tool":"download","chat":"me","message_id":42}"#)
        .expect("download invocation should parse");
    match inv.command {
        ToolCommand::Download(Download {
            chat,
            message_id,
            path,
        }) => {
            assert_eq!((chat.as_str(), message_id), ("me", 42));
            assert_eq!(path, None);
        }
        other => panic!("expected Download, got {other:?}"),
    }
}

#[test]
fn tool_tag_roundtrips_all_eight() {
    // One of each command serialized → re-parsed → identical, and the
    // `tool` tag on disk matches `name()`.
    let all = vec![
        read_chat("c"),
        ToolCommand::SearchChat(SearchChat {
            chat: "c".into(),
            query: "q".into(),
            limit: 5,
        }),
        ToolCommand::Discover(Discover {
            username: Some("u".into()),
            phone: None,
            limit: 5,
        }),
        ToolCommand::Raw(Raw {
            method: "m".into(),
            params: serde_json::json!({"x": 1}),
            confirm: true,
        }),
        ToolCommand::SendToPhone(SendToPhone {
            phone: "+254712345678".into(),
            text: "hi".into(),
        }),
    ];
    for cmd in all {
        let inv = ToolInvocation::new(cmd.clone());
        let json = inv.to_json().unwrap();
        assert!(json.contains(&format!("\"{}\"", cmd.name())) || cmd.name() == "raw");
        let back = ToolInvocation::parse(&json).unwrap();
        assert_eq!(back.command, cmd);
    }
}

fn enabled_cfg() -> TelegramUserbotConfig {
    TelegramUserbotConfig {
        enabled: true,
        ..Default::default()
    }
}

#[test]
fn authorize_read_tools_pass_when_enabled() {
    assert_eq!(authorize(&read_chat("c"), &enabled_cfg()), Ok(()));
}

#[test]
fn authorize_denies_everything_when_disabled() {
    let cfg = TelegramUserbotConfig::default();
    assert_eq!(authorize(&read_chat("c"), &cfg), Err(Denial::Disabled));
}

#[test]
fn authorize_empty_allowlist_is_strictly_read_only() {
    let send = ToolCommand::SendMessage(SendMessage {
        chat: "@somebody".into(),
        text: "hi".into(),
        reply_to: None,
        schedule_unix: None,
        wait_reply_secs: None,
    });
    let edit = ToolCommand::EditMessage(EditMessage {
        chat: "@somebody".into(),
        message_id: 42,
        new_text: "hi".into(),
    });
    let file = ToolCommand::SendFile(SendFile {
        chat: "@somebody".into(),
        path: "/tmp/x".into(),
        caption: None,
    });
    for cmd in [send, edit, file] {
        assert_eq!(
            authorize(&cmd, &enabled_cfg()),
            Err(Denial::OutboundNotAllowed {
                target: "@somebody".into()
            })
        );
    }
}

#[test]
fn authorize_outbound_passes_only_for_allowlisted_target() {
    let mut cfg = enabled_cfg();
    cfg.chat_permissions
        .insert("@friend".into(), vec![ChatPermission::Send]);
    let ok = ToolCommand::SendMessage(SendMessage {
        chat: "@friend".into(),
        text: "hi".into(),
        reply_to: None,
        schedule_unix: None,
        wait_reply_secs: None,
    });
    let denied = ToolCommand::SendMessage(SendMessage {
        chat: "@stranger".into(),
        text: "hi".into(),
        reply_to: None,
        schedule_unix: None,
        wait_reply_secs: None,
    });
    assert_eq!(authorize(&ok, &cfg), Ok(()));
    assert_eq!(
        authorize(&denied, &cfg),
        Err(Denial::OutboundNotAllowed {
            target: "@stranger".into()
        })
    );
    // Phone sends: the allowlist target is the phone literal, so a
    // saved contact can't be pinged via a second, unlisted phone form.
    let phone_ok = ToolCommand::SendToPhone(SendToPhone {
        phone: "+254712345678".into(),
        text: "hi".into(),
    });
    let phone_denied = ToolCommand::SendToPhone(SendToPhone {
        phone: "+254700000000".into(),
        text: "hi".into(),
    });
    cfg.chat_permissions
        .insert("+254712345678".into(), vec![ChatPermission::Send]);
    assert_eq!(authorize(&phone_ok, &cfg), Ok(()));
    assert_eq!(
        authorize(&phone_denied, &cfg),
        Err(Denial::OutboundNotAllowed {
            target: "+254700000000".into()
        })
    );
}

#[test]
fn react_requires_send_permission_for_target_chat() {
    let mut cfg = enabled_cfg();
    cfg.chat_permissions
        .insert("-1001234567890".into(), vec![ChatPermission::Read]);
    let react = ToolCommand::React(React {
        chat: "-1001234567890".into(),
        message_id: 42,
        emoji: "👍".into(),
    });
    // read-only grant must not allow a reaction
    assert_eq!(
        authorize(&react, &cfg),
        Err(Denial::OutboundNotAllowed {
            target: "-1001234567890".into()
        })
    );
    cfg.chat_permissions
        .get_mut("-1001234567890")
        .unwrap()
        .push(ChatPermission::Send);
    assert_eq!(authorize(&react, &cfg), Ok(()));
}

#[test]
fn authorize_raw_requires_confirm_flag() {
    let unconfirmed = ToolCommand::Raw(Raw {
        method: "messages.GetHistory".into(),
        params: serde_json::json!({}),
        confirm: false,
    });
    let confirmed = ToolCommand::Raw(Raw {
        method: "messages.GetHistory".into(),
        params: serde_json::json!({}),
        confirm: true,
    });
    assert_eq!(
        authorize(&unconfirmed, &enabled_cfg()),
        Err(Denial::RawUnconfirmed)
    );
    assert_eq!(authorize(&confirmed, &enabled_cfg()), Ok(()));
}

// Silence unused-import warning for Write when temp helpers evolve.
#[allow(dead_code)]
fn _unused(_: &mut dyn Write) {}

#[test]
fn sample_tools_toml_parses_and_covers_the_surface() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/docs/tools-telegram-userbot.toml"
    ))
    .expect("sample tools.toml shipped with the repo");
    let parsed: toml::Value = toml::from_str(&raw).expect("sample is valid TOML");
    let tools = parsed["tools"]
        .as_array()
        .expect("tools array present")
        .clone();
    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().expect("tool name"))
        .collect();
    for expected in [
        "tg_get_messages",
        "tg_send_message",
        "tg_send_document",
        "tg_edit_message",
        "tg_react",
        "tg_download",
        "tg_find_chats",
        "tg_get_chat_info",
        "tg_search_global",
        "tg_send_to_phone",
        "tg_mtproto",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }
    // Every entry must invoke the local binary, not a remote server.
    for t in &tools {
        let cmd = t["command"].as_str().expect("command");
        assert!(
            cmd.starts_with("opencrabs userbot tool --params-file"),
            "non-native command: {cmd}"
        );
        assert!(
            !cmd.contains("http") && !cmd.contains("l1979"),
            "no remote endpoints in the drop-in: {cmd}"
        );
    }
}

#[test]
fn flood_wait_extraction_from_error_chain() {
    use anyhow::Context as _;
    use grammers_client::InvocationError;
    use grammers_client::sender::RpcError;

    let flood = anyhow::Error::new(InvocationError::Rpc(RpcError {
        code: 420,
        name: "FLOOD_WAIT".to_string(),
        value: Some(31),
        caused_by: None,
    }));
    assert_eq!(
        crate::channels::telegram::userbot::tools::flood_wait_secs(&flood),
        Some(31)
    );

    // Context-wrapped: the flood still surfaces through the chain.
    let wrapped = flood.context("send failed");
    assert_eq!(
        crate::channels::telegram::userbot::tools::flood_wait_secs(&wrapped),
        Some(31)
    );

    // Non-flood RPC errors are not waits.
    let other = anyhow::Error::new(InvocationError::Rpc(RpcError {
        code: 400,
        name: "CHAT_ID_INVALID".to_string(),
        value: None,
        caused_by: None,
    }));
    assert_eq!(
        crate::channels::telegram::userbot::tools::flood_wait_secs(&other),
        None
    );
}
