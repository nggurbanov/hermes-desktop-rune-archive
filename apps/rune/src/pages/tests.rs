use super::*;
fn scope(connection: u64, profile: &str) -> DraftScope {
    DraftScope {
        connection,
        profile: profile.into(),
        session: None,
    }
}
fn reply(id: u64, progress: bool, operation: &str, owner: Owner) -> PageReply {
    PageReply {
        progress,
        id,
        operation: operation.into(),
        profile: "explicit-profile".into(),
        owner,
        result: Ok(Value::Null),
    }
}
#[test]
fn workspace_replies_fence_request_identity_and_ambient_owner_without_consuming_progress() {
    let (tx, receive) = mpsc::channel();
    let mut network = PageNetwork {
        receive: Some(receive),
        ..Default::default()
    };
    let old = Owner::from(&scope(1, "work"));
    let active = Owner::from(&scope(2, "work"));
    network
        .pending
        .insert("settings.billing".into(), (2, active.clone()));
    tx.send(reply(1, false, "settings.billing", old)).unwrap();
    tx.send(reply(2, true, "settings.billing", active.clone()))
        .unwrap();
    let replies = network.poll(&scope(2, "work"));
    assert_eq!(replies.len(), 1);
    assert!(replies[0].progress);
    assert!(network.pending.contains_key("settings.billing"));
    tx.send(reply(2, false, "settings.billing", active.clone()))
        .unwrap();
    assert_eq!(network.poll(&scope(2, "other")).len(), 0);
    assert!(network.pending.is_empty());
    network
        .pending
        .insert("settings.config".into(), (3, active.clone()));
    network.invalidate_prefix("settings.");
    tx.send(reply(3, false, "settings.config", active)).unwrap();
    assert!(network.poll(&scope(2, "work")).is_empty());
}

#[test]
fn a_frozen_plugin_target_changes_only_the_transport_not_the_ambient_reply_owner() {
    let (send, receive) = mpsc::channel();
    let mut network = PageNetwork {
        send: Some(send),
        ..Default::default()
    };
    let ambient = Connection::Url {
        endpoint: "http://ambient.invalid".into(),
        token: Some("ambient synthetic token".into()),
    };
    let target = Connection::Url {
        endpoint: "http://target.invalid".into(),
        token: Some("target synthetic token".into()),
    };
    network
        .request(
            &scope(7, "same-name"),
            ambient,
            "plugin-host.synthetic",
            PageRequest::get("/api/config")
                .profile("same-name")
                .connection(target),
        )
        .unwrap();
    let work = receive.recv().unwrap();
    assert!(
        matches!(work.connection,Connection::Url{ref endpoint,ref token} if endpoint=="http://target.invalid" && token.as_deref()==Some("target synthetic token"))
    );
    assert_eq!(work.owner.connection, 7);
    assert_eq!(work.owner.profile, "same-name");
}

#[test]
fn retired_reads_are_skipped_before_io_but_already_queued_writes_keep_their_owner() {
    let (send, receive) = mpsc::channel();
    let mut network = PageNetwork {
        send: Some(send),
        ..Default::default()
    };
    let dir = std::env::temp_dir().join(format!("hermes-read-queue-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("notes.txt");
    std::fs::write(&path, "old").unwrap();
    let local = Connection::Local { home: dir.clone() };
    network
        .request(
            &scope(1, "work"),
            local.clone(),
            "files.read",
            PageRequest::local_fs("/api/fs/read-text", serde_json::json!({"path":path})),
        )
        .unwrap();
    network
        .request(
            &scope(1, "work"),
            local,
            "files.write",
            PageRequest::local_fs(
                "/api/fs/write-text",
                serde_json::json!({"path":path,"expected":"old","content":"captured edit"}),
            ),
        )
        .unwrap();
    network.invalidate();
    let read = receive.recv().unwrap();
    let write = receive.recv().unwrap();
    assert!(read.run(|_| {}).is_none());
    assert!(write.run(|_| {}).unwrap().is_ok());
    assert_eq!(write.owner.profile, "work");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "captured edit");
    std::fs::remove_dir_all(dir).unwrap();
}
