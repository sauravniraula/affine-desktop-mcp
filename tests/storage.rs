mod common;
use affine_local_mcp::storage::data_dir_for;
use common::{WORKSPACE, db_path, fixture};
use rusqlite::{Connection, params};
use serde_json::json;
use std::{collections::HashSet, fs, path::Path};

#[test]
fn platform_data_directories() {
    let home = Path::new("home");
    assert_eq!(
        data_dir_for("macos", home, None, None),
        home.join("Library/Application Support/AFFiNE")
    );
    assert_eq!(
        data_dir_for("windows", home, None, None),
        home.join("AppData/Roaming/AFFiNE")
    );
    assert_eq!(
        data_dir_for("windows", home, Some("roaming".into()), None),
        Path::new("roaming/AFFiNE")
    );
    assert_eq!(
        data_dir_for("linux", home, None, None),
        home.join(".config/AFFiNE")
    );
    assert_eq!(
        data_dir_for("linux", home, None, Some("xdg".into())),
        Path::new("xdg/AFFiNE")
    );
}

#[test]
fn reads_independent_yjs_snapshot_and_unmerged_update_without_mutating_database() {
    let (_dir, storage) = fixture();
    let before = fs::read(db_path(&storage)).unwrap();
    let workspaces = storage.list_workspaces().unwrap();
    assert_eq!(workspaces["workspaces"][0]["name"], "Fixture Workspace");
    assert_eq!(workspaces["workspaces"][0]["documentCount"], 1);
    let list = storage.list_documents(WORKSPACE, 0, 1).unwrap();
    assert_eq!(list["total"], 1);
    assert_eq!(list["documents"][0]["id"], "fixture-page");
    assert_eq!(list["hasMore"], false);
    let doc = storage
        .read_document(WORKSPACE, "fixture-page", 0, 100_000)
        .unwrap();
    let text = doc["markdown"].as_str().unwrap();
    assert!(text.contains("Snapshot plus update 🌏"));
    assert!(text.contains("| Header |\n| --- |\n| Cell value |"));
    assert!(text.contains("Canvas needle"));
    assert_eq!(doc["blockCount"], 5);
    assert_eq!(fs::read(db_path(&storage)).unwrap(), before);
    assert!(
        storage
            .read_document(WORKSPACE, "fixture-trash", 0, 100)
            .is_err()
    );
    assert!(storage.read_document(WORKSPACE, "orphan", 0, 100).is_err());
    assert!(
        storage
            .read_document("../test-workspace", "fixture-page", 0, 100)
            .is_err()
    );
    let result = storage.search(WORKSPACE, "CANVAS NEEDLE", 0, 10).unwrap();
    assert_eq!(result["total"], 1);
    assert_eq!(result["partial"], false);
    assert!(storage.search(WORKSPACE, "  ", 0, 10).is_err());
}

#[test]
fn unicode_pagination_and_workspace_allowlist() {
    let (_dir, mut storage) = fixture();
    let full = storage
        .read_document(WORKSPACE, "fixture-page", 0, 100_000)
        .unwrap();
    let mut result = String::new();
    let mut offset = 0;
    loop {
        let page = storage
            .read_document(WORKSPACE, "fixture-page", offset, 7)
            .unwrap();
        result.push_str(page["markdown"].as_str().unwrap());
        let Some(next) = page["nextOffset"].as_u64() else {
            break;
        };
        offset = next as usize;
    }
    assert_eq!(result, full["markdown"].as_str().unwrap());
    assert!(
        storage
            .read_document(WORKSPACE, "fixture-page", 0, 0)
            .is_err()
    );
    assert!(storage.list_documents(WORKSPACE, 0, 201).is_err());
    assert_eq!(
        storage.list_documents(WORKSPACE, usize::MAX, 100).unwrap()["documents"],
        json!([])
    );
    storage.allowlist = Some(HashSet::new());
    assert_eq!(storage.list_workspaces().unwrap()["workspaces"], json!([]));
    assert!(storage.list_documents(WORKSPACE, 0, 10).is_err());
}

#[test]
fn corrupt_and_incomplete_updates_fail_without_silently_returning_partial_text() {
    let (_dir, storage) = fixture();
    let db = Connection::open(db_path(&storage)).unwrap();
    db.execute("DELETE FROM snapshots WHERE doc_id='fixture-page'", [])
        .unwrap();
    assert!(
        storage
            .read_document(WORKSPACE, "fixture-page", 0, 100)
            .unwrap_err()
            .to_string()
            .contains("Incomplete Yjs")
    );
    let result = storage.search(WORKSPACE, "needle", 0, 20).unwrap();
    assert_eq!(result["partial"], true);
    assert_eq!(result["warnings"].as_array().unwrap().len(), 1);
    db.execute(
        "INSERT INTO updates VALUES (?1,?2,?3)",
        params!["fixture-page", vec![255u8], "2026-01-01 00:00:00.002"],
    )
    .unwrap();
    assert!(
        storage
            .read_document(WORKSPACE, "fixture-page", 0, 100)
            .is_err()
    );
}

#[test]
fn creation_is_disabled_by_default_and_schema_mismatch_is_rejected() {
    let (_dir, mut storage) = fixture();
    storage.writes = false;
    let before = fs::read(db_path(&storage)).unwrap();
    assert!(
        storage
            .create(WORKSPACE, "Test", "Content", "page", None)
            .unwrap_err()
            .to_string()
            .contains("disabled")
    );
    assert_eq!(fs::read(db_path(&storage)).unwrap(), before);
    assert!(!storage.data_dir.join("mcp-backups").exists());
    let db = Connection::open(db_path(&storage)).unwrap();
    db.execute("DROP TABLE clocks", []).unwrap();
    assert!(
        storage
            .list_documents(WORKSPACE, 0, 100)
            .unwrap_err()
            .to_string()
            .contains("schema")
    );
}

#[test]
fn symlinked_workspace_outside_the_allowed_root_is_not_exposed() {
    let (_dir, storage) = fixture();
    let outside = tempfile::tempdir().unwrap();
    fs::copy(db_path(&storage), outside.path().join("storage.db")).unwrap();
    let link = storage.data_dir.join("workspaces/local/escape");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    #[cfg(windows)]
    if std::os::windows::fs::symlink_dir(outside.path(), &link).is_err() {
        return;
    }
    assert_eq!(
        storage.list_workspaces().unwrap()["workspaces"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
