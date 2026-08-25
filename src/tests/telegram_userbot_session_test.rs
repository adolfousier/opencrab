//! Receive-only userbot session persistence tests.

use grammers_session::Session as _;
use grammers_session::types::{DcOption, PeerId, PeerInfo, UpdateState};

use crate::channels::telegram::userbot::session::FileSession;

#[tokio::test]
async fn round_trips_auth_key_peers_and_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("session.json");
    let session = FileSession::load(&path).expect("fresh load");
    let key = [7u8; 256];
    let dc = DcOption {
        id: 2,
        ipv4: "127.0.0.1:443".parse().expect("ipv4"),
        ipv6: "[::1]:443".parse().expect("ipv6"),
        auth_key: Some(key),
    };

    session.set_dc_option(&dc).await.expect("set dc");
    session
        .cache_peer(&PeerInfo::User {
            id: 42,
            auth: None,
            bot: Some(false),
            is_self: Some(true),
        })
        .await
        .expect("cache peer");
    session
        .set_update_state(UpdateState::Primary {
            pts: 111,
            date: 222,
            seq: 3,
        })
        .await
        .expect("set state");
    session.save().expect("save");

    let reloaded = FileSession::load(&path).expect("reload");
    assert_eq!(
        reloaded
            .dc_option(2)
            .expect("dc read")
            .expect("dc")
            .auth_key,
        Some(key)
    );
    assert!(
        reloaded
            .peer(PeerId::user_unchecked(42))
            .await
            .expect("peer read")
            .is_some()
    );
    let state = reloaded.updates_state().await.expect("state");
    assert_eq!((state.pts, state.date, state.seq), (111, 222, 3));
}

#[cfg(unix)]
#[test]
fn file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("session.json");
    FileSession::load(&path)
        .expect("load")
        .save()
        .expect("save");
    let mode = std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}
