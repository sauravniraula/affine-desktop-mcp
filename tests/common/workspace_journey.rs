use crate::{Client, common::WORKSPACE, document_journey::stored_doc};
use affine_local_mcp::{
    documents,
    storage::Storage,
    workspaces::{CreateWorkspace, DeleteWorkspace},
};
use rusqlite::Connection;
use serde_json::json;
use std::{collections::HashSet, fs};

pub fn run(client: &mut Client, storage: &Storage) {
    // A prior process can die in a hidden stage without reserving the requested ID.
    let interrupted = storage.data_dir.join("mcp-staging/.mcp-create-interrupted");
    fs::create_dir_all(&interrupted).unwrap();
    fs::write(
        interrupted.join("unrelated-recovery-data"),
        "Must not be removed",
    )
    .unwrap();
    let created_args = json!({"workspaceId":"created-workspace","name":"New workspace 日本語"});
    let created = client.success("affine_create_workspace", created_args.clone());
    assert_eq!(
        fs::read_to_string(interrupted.join("unrelated-recovery-data")).unwrap(),
        "Must not be removed"
    );
    assert_eq!(created["alreadyExists"], false);
    assert_eq!(created["reopenAffine"], true);
    let list = client.success("affine_list_workspaces", json!({}));
    let row = list["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "created-workspace")
        .unwrap();
    assert_eq!(row["name"], "New workspace 日本語");
    assert_eq!(row["documentCount"], 0);
    let backup = Connection::open(created["backupPath"].as_str().unwrap()).unwrap();
    assert_eq!(
        documents::root_json(&stored_doc(&backup, "created-workspace"), "meta").unwrap()["name"],
        "New workspace 日本語"
    );
    assert_eq!(
        client.success(
            "affine_list_documents",
            json!({"workspaceId":"created-workspace"})
        )["total"],
        0
    );
    client.success("affine_create_document",json!({"workspaceId":"created-workspace","documentId":"new-doc","title":"First page","content":"Persisted inside the new workspace"}));
    client.success("affine_manage_folder",json!({"workspaceId":"created-workspace","action":"create","kind":"folder","name":"Project"}));
    client.success(
        "affine_manage_tag",
        json!({"workspaceId":"created-workspace","action":"create","name":"Project"}),
    );
    assert_eq!(
        client.success("affine_create_workspace", created_args.clone())["alreadyExists"],
        true
    );
    assert_eq!(
        client.call(
            "affine_create_workspace",
            json!({"workspaceId":"created-workspace","name":"Conflict"})
        )["isError"],
        true
    );
    for args in [
        json!({"workspaceId":"created-workspace","confirmName":"New workspace 日本語","confirm":false}),
        json!({"workspaceId":"created-workspace","confirmName":"Wrong name","confirm":true}),
        json!({"workspaceId":"../created-workspace","confirmName":"New workspace 日本語","confirm":true}),
    ] {
        assert_eq!(
            client.call("affine_delete_workspace", args)["isError"],
            true
        );
    }
    let workspace = storage.data_dir.join("workspaces/local/created-workspace");
    fs::write(
        workspace.join("retained-sidecar.txt"),
        "Keep all workspace files",
    )
    .unwrap();
    let db = Connection::open(workspace.join("storage.db")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
    db.execute(
        "INSERT INTO blobs(key,data,mime,size) VALUES ('retained-blob',?1,'text/plain',4)",
        [b"blob".as_slice()],
    )
    .unwrap();
    drop(db);
    let deleted_dir = storage
        .data_dir
        .join("deleted-workspaces/created-workspace");
    fs::create_dir(&deleted_dir).unwrap();
    assert_eq!(client.call("affine_delete_workspace",json!({"workspaceId":"created-workspace","confirmName":"New workspace 日本語","confirm":true}))["isError"],true);
    assert!(workspace.join("storage.db").is_file());
    fs::remove_dir(&deleted_dir).unwrap();
    let delete_args = json!({"workspaceId":"created-workspace","confirmName":"New workspace 日本語","confirm":true});
    let deleted = client.success("affine_delete_workspace", delete_args.clone());
    assert_eq!(deleted["alreadyDeleted"], false);
    assert!(!workspace.exists());
    assert_eq!(
        fs::read_to_string(deleted_dir.join("retained-sidecar.txt")).unwrap(),
        "Keep all workspace files"
    );
    for path in [
        deleted["backupPath"].as_str().unwrap(),
        deleted_dir.join("storage.db").to_str().unwrap(),
    ] {
        let db = Connection::open(path).unwrap();
        assert_eq!(
            db.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        assert_eq!(
            documents::render(&stored_doc(&db, "new-doc"))
                .unwrap()
                .markdown,
            "# First page\n\nPersisted inside the new workspace"
        );
        assert_eq!(
            db.query_row(
                "SELECT data FROM blobs WHERE key='retained-blob'",
                [],
                |row| row.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
            b"blob"
        );
    }
    assert!(
        !client.success("affine_list_workspaces", json!({}))["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "created-workspace")
    );
    assert_eq!(
        client.call(
            "affine_list_documents",
            json!({"workspaceId":"created-workspace"})
        )["isError"],
        true
    );
    assert_eq!(
        client.success("affine_delete_workspace", delete_args)["alreadyDeleted"],
        true
    );
    assert_eq!(
        client.call("affine_create_workspace", created_args)["isError"],
        true
    );
    assert!(deleted_dir.join("storage.db").is_file());

    let mut read_only = Client::start(&storage.data_dir, false);
    assert_eq!(
        read_only.call("affine_create_workspace", json!({"name":"Blocked"}))["isError"],
        true
    );
    assert_eq!(
        read_only.call(
            "affine_delete_workspace",
            json!({"workspaceId":WORKSPACE,"confirmName":"Fixture Workspace","confirm":true})
        )["isError"],
        true
    );
    let mut restricted = storage.clone();
    restricted.allowlist = Some(HashSet::from([WORKSPACE.to_owned()]));
    assert!(
        restricted
            .create_workspace(&CreateWorkspace {
                name: "Blocked".into(),
                workspace_id: Some("not-allowed".into())
            })
            .is_err()
    );
    assert!(
        restricted
            .delete_workspace(&DeleteWorkspace {
                workspace_id: "created-workspace".into(),
                confirm_name: "New workspace 日本語".into(),
                confirm: true
            })
            .is_err()
    );

    // A failed creation leaves no discoverable or reserved workspace behind.
    let fresh = tempfile::tempdir().unwrap();
    fs::write(fresh.path().join("mcp-backups"), "blocked backup directory").unwrap();
    let fresh_storage = Storage::new(fresh.path().into(), true);
    assert!(
        fresh_storage
            .create_workspace(&CreateWorkspace {
                name: "Failed".into(),
                workspace_id: Some("failed-workspace".into())
            })
            .is_err()
    );
    assert!(
        !fresh
            .path()
            .join("workspaces/local/failed-workspace")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(fresh.path().join("mcp-backups")).unwrap(),
        "blocked backup directory"
    );

    // Separate MCP processes use the OS lock, not a shared in-process mutex.
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let data_dir = storage.data_dir.clone();
            std::thread::spawn(move || {
                let mut client = Client::start(&data_dir, true);
                client.success(
                    "affine_create_workspace",
                    json!({"name":"Concurrent workspace","workspaceId":"guard-workspace"}),
                )
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|result| result["alreadyExists"] == false)
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| result["alreadyExists"] == true)
            .count(),
        1
    );

    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        let fresh = tempfile::tempdir().unwrap();
        fs::create_dir(fresh.path().join("workspaces")).unwrap();
        std::os::unix::fs::symlink(outside.path(), fresh.path().join("workspaces/local")).unwrap();
        let unsafe_storage = Storage::new(fresh.path().into(), true);
        assert!(
            unsafe_storage
                .create_workspace(&CreateWorkspace {
                    name: "Blocked".into(),
                    workspace_id: Some("escape".into())
                })
                .is_err()
        );
        assert!(!outside.path().join("escape").exists());
    }
}
