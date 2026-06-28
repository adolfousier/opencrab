use crate::channels::whatsapp::store::Store;
use wacore::appstate::processor::AppStateMutationMAC;
use wacore::store::traits::{
    AppStateSyncKey, AppSyncStore, DeviceListRecord, DeviceStore, LidPnMappingEntry, ProtocolStore,
    SignalStore,
};

async fn test_store() -> Store {
    Store::new(":memory:").await.unwrap()
}

#[tokio::test]
async fn test_identity_roundtrip() {
    let store = test_store().await;
    let key = [42u8; 32];
    store
        .put_identity("alice@s.whatsapp.net", key)
        .await
        .unwrap();

    let loaded = store.load_identity("alice@s.whatsapp.net").await.unwrap();
    assert_eq!(loaded.unwrap(), key);
}

#[tokio::test]
async fn test_identity_missing() {
    let store = test_store().await;
    let loaded = store.load_identity("nobody").await.unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn test_identity_delete() {
    let store = test_store().await;
    store.put_identity("bob", [1u8; 32]).await.unwrap();
    store.delete_identity("bob").await.unwrap();
    assert!(store.load_identity("bob").await.unwrap().is_none());
}

#[tokio::test]
async fn test_session_roundtrip() {
    let store = test_store().await;
    let data = b"session-bytes";
    store.put_session("addr1", data).await.unwrap();
    let loaded = store.get_session("addr1").await.unwrap().unwrap();
    assert_eq!(&loaded[..], &data[..]);
    assert!(store.has_session("addr1").await.unwrap());
    assert!(!store.has_session("nonexistent").await.unwrap());
}

#[tokio::test]
async fn test_prekey_roundtrip() {
    let store = test_store().await;
    store.store_prekey(1, b"prekey-data", false).await.unwrap();
    let loaded = store.load_prekey(1).await.unwrap().unwrap();
    assert_eq!(&loaded[..], &b"prekey-data"[..]);
    store.remove_prekey(1).await.unwrap();
    assert!(store.load_prekey(1).await.unwrap().is_none());
}

#[tokio::test]
async fn test_signed_prekey_roundtrip() {
    let store = test_store().await;
    store.store_signed_prekey(10, b"spk-data").await.unwrap();
    let loaded = store.load_signed_prekey(10).await.unwrap().unwrap();
    assert_eq!(loaded, b"spk-data");

    let all = store.load_all_signed_prekeys().await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0], (10, b"spk-data".to_vec()));

    store.remove_signed_prekey(10).await.unwrap();
    assert!(store.load_signed_prekey(10).await.unwrap().is_none());
}

#[tokio::test]
async fn test_sender_key_roundtrip() {
    let store = test_store().await;
    store
        .put_sender_key("group::sender", b"sk-data")
        .await
        .unwrap();
    let loaded = store
        .get_sender_key("group::sender")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded, b"sk-data");
    store.delete_sender_key("group::sender").await.unwrap();
    assert!(
        store
            .get_sender_key("group::sender")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn test_app_sync_key_roundtrip() {
    let store = test_store().await;
    let key = AppStateSyncKey {
        key_data: vec![1, 2, 3],
        fingerprint: vec![4, 5],
        timestamp: 12345,
    };
    store.set_sync_key(b"kid1", key.clone()).await.unwrap();
    let loaded = store.get_sync_key(b"kid1").await.unwrap().unwrap();
    assert_eq!(loaded.key_data, key.key_data);
    assert_eq!(loaded.timestamp, key.timestamp);
}

#[tokio::test]
async fn test_version_default() {
    let store = test_store().await;
    let state = store.get_version("critical_block").await.unwrap();
    assert_eq!(state.version, 0);
}

#[tokio::test]
async fn test_sender_key_devices() {
    let store = test_store().await;

    // Set statuses for a group, then read them back.
    store
        .set_sender_key_status(
            "group1",
            &[
                ("dev1@s.whatsapp.net", true),
                ("dev2@s.whatsapp.net", false),
            ],
        )
        .await
        .unwrap();
    let mut devices = store.get_sender_key_devices("group1").await.unwrap();
    devices.sort();
    assert_eq!(
        devices,
        vec![
            ("dev1@s.whatsapp.net".to_string(), true),
            ("dev2@s.whatsapp.net".to_string(), false),
        ]
    );

    // Flip has_key for an existing device (upsert).
    store
        .set_sender_key_status("group1", &[("dev2@s.whatsapp.net", true)])
        .await
        .unwrap();
    let devices = store.get_sender_key_devices("group1").await.unwrap();
    assert!(
        devices
            .iter()
            .any(|(jid, has_key)| jid == "dev2@s.whatsapp.net" && *has_key)
    );

    // Clear a group removes only that group's rows.
    store
        .set_sender_key_status("group2", &[("dev1@s.whatsapp.net", true)])
        .await
        .unwrap();
    store.clear_sender_key_devices("group1").await.unwrap();
    assert!(
        store
            .get_sender_key_devices("group1")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.get_sender_key_devices("group2").await.unwrap().len(),
        1
    );

    // delete_sender_key_device_rows removes a device across all groups.
    store
        .set_sender_key_status(
            "group1",
            &[("dev1@s.whatsapp.net", true), ("dev3@s.whatsapp.net", true)],
        )
        .await
        .unwrap();
    store
        .delete_sender_key_device_rows(&["dev1@s.whatsapp.net"])
        .await
        .unwrap();
    assert!(
        store
            .get_sender_key_devices("group1")
            .await
            .unwrap()
            .iter()
            .all(|(jid, _)| jid != "dev1@s.whatsapp.net")
    );
    assert!(
        store
            .get_sender_key_devices("group2")
            .await
            .unwrap()
            .is_empty()
    );

    // Empty input is a no-op.
    store.delete_sender_key_device_rows(&[]).await.unwrap();

    // clear_all wipes everything.
    store.clear_all_sender_key_devices().await.unwrap();
    assert!(
        store
            .get_sender_key_devices("group1")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn test_lid_mapping() {
    let store = test_store().await;
    let entry = LidPnMappingEntry {
        lid: "lid123".into(),
        phone_number: "+15551234".into(),
        created_at: 100,
        updated_at: 200,
        learning_source: "test".into(),
    };
    store.put_lid_mapping(&entry).await.unwrap();

    let by_lid = store.get_lid_mapping("lid123").await.unwrap().unwrap();
    assert_eq!(by_lid.phone_number, "+15551234");

    let by_phone = store.get_pn_mapping("+15551234").await.unwrap().unwrap();
    assert_eq!(by_phone.lid, "lid123");

    let all = store.get_all_lid_mappings().await.unwrap();
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn test_base_key_collision() {
    let store = test_store().await;
    store.save_base_key("addr", "msg1", b"key1").await.unwrap();
    assert!(
        store
            .has_same_base_key("addr", "msg1", b"key1")
            .await
            .unwrap()
    );
    assert!(
        !store
            .has_same_base_key("addr", "msg1", b"key2")
            .await
            .unwrap()
    );
    assert!(
        !store
            .has_same_base_key("addr", "msg2", b"key1")
            .await
            .unwrap()
    );
    store.delete_base_key("addr", "msg1").await.unwrap();
    assert!(
        !store
            .has_same_base_key("addr", "msg1", b"key1")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_delete_devices() {
    let store = test_store().await;
    let record = DeviceListRecord {
        user: "user1".into(),
        devices: vec![],
        timestamp: 1,
        phash: None,
        raw_id: None,
    };
    store.update_device_list(record).await.unwrap();
    assert!(store.get_devices("user1").await.unwrap().is_some());
    store.delete_devices("user1").await.unwrap();
    assert!(store.get_devices("user1").await.unwrap().is_none());
}

#[tokio::test]
async fn test_device_store_create_exists() {
    let store = test_store().await;
    assert!(!store.exists().await.unwrap());
    let id = store.create().await.unwrap();
    assert_eq!(id, 1);
    // create doesn't persist — only save does
    assert!(!store.exists().await.unwrap());
}

#[tokio::test]
async fn test_mutation_macs() {
    let store = test_store().await;
    let macs = vec![
        AppStateMutationMAC {
            index_mac: vec![1, 2],
            value_mac: vec![3, 4],
        },
        AppStateMutationMAC {
            index_mac: vec![5, 6],
            value_mac: vec![7, 8],
        },
    ];
    store
        .put_mutation_macs("critical_block", 1, &macs)
        .await
        .unwrap();

    let v = store
        .get_mutation_mac("critical_block", &[1, 2])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v, vec![3, 4]);

    store
        .delete_mutation_macs("critical_block", &[vec![1, 2]])
        .await
        .unwrap();
    assert!(
        store
            .get_mutation_mac("critical_block", &[1, 2])
            .await
            .unwrap()
            .is_none()
    );
    // Second one still there
    assert!(
        store
            .get_mutation_mac("critical_block", &[5, 6])
            .await
            .unwrap()
            .is_some()
    );
}
