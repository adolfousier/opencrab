use crate::channels::telegram::cowork::*;

#[test]
fn parse_startgroup_valid() {
    assert_eq!(parse_startgroup_param("cowork_abc123"), Some("abc123"));
}

#[test]
fn parse_startgroup_not_cowork() {
    assert_eq!(parse_startgroup_param("other_param"), None);
}

#[test]
fn parse_startgroup_empty() {
    assert_eq!(parse_startgroup_param(""), None);
}

#[test]
fn parse_startgroup_just_prefix() {
    assert_eq!(parse_startgroup_param("cowork_"), None);
}

#[test]
fn is_cowork_session_true() {
    assert!(is_cowork_session("cowork_xxx"));
}

#[test]
fn is_cowork_session_false() {
    assert!(!is_cowork_session("other"));
}

#[test]
fn build_deep_link_format() {
    // #709: the link requests admin rights inline so the bot joins promoted.
    let link = build_cowork_deep_link("mybot", "abc123");
    assert_eq!(
        link,
        "https://t.me/mybot?startgroup=cowork_abc123&admin=invite_users+delete_messages+pin_messages+manage_chat"
    );
}

#[test]
fn build_deep_link_requests_invite_users() {
    // create_chat_invite_link needs can_invite_users; it must be in the request.
    let link = build_cowork_deep_link("mybot", "abc123");
    let (_, admin) = link.split_once("&admin=").expect("admin param present");
    assert!(admin.split('+').any(|r| r == "invite_users"));
}

#[test]
fn build_deep_link_with_bot_suffix() {
    let link = build_cowork_deep_link("team_crab_bot", "xyz");
    assert!(link.starts_with("https://t.me/team_crab_bot?startgroup=cowork_xyz&admin="));
}

#[test]
fn cowork_state_lifecycle() {
    let state = CoworkState::new(123, 456, "abc".to_string());
    assert_eq!(state.user_id, 123);
    assert_eq!(state.chat_id, 456);
    assert_eq!(state.session_id, "abc");
    assert!(!state.is_expired());
}

#[test]
#[cfg(feature = "whatsapp")]
fn invite_qr_generation() {
    let result = build_invite_qr("https://t.me/+AbCdEfGh");
    assert!(result.is_some());
    let (bytes, path) = result.unwrap();
    // PNG magic number
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert!(path.exists());
    // Cleanup
    let _ = std::fs::remove_file(path);
}
