use affine_local_mcp::storage::Storage;
use rusqlite::{Connection, params};
use std::{fs, path::PathBuf};
use tempfile::TempDir;

pub const WORKSPACE: &str = "test-workspace";

pub fn fixture() -> (TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("AFFiNE data with spaces 日本語");
    let workspace = data.join("workspaces/local").join(WORKSPACE);
    fs::create_dir_all(&workspace).unwrap();
    let db = Connection::open(workspace.join("storage.db")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE meta(space_id TEXT PRIMARY KEY NOT NULL);
        CREATE TABLE snapshots(doc_id TEXT PRIMARY KEY NOT NULL, data BLOB NOT NULL, created_at TEXT DEFAULT CURRENT_TIMESTAMP NOT NULL, updated_at TEXT NOT NULL);
        CREATE TABLE updates(doc_id TEXT NOT NULL, data BLOB NOT NULL, created_at TEXT NOT NULL, PRIMARY KEY(doc_id,created_at));
        CREATE TABLE clocks(doc_id TEXT PRIMARY KEY NOT NULL,timestamp TEXT NOT NULL);
        CREATE TABLE blobs(key TEXT PRIMARY KEY NOT NULL,data BLOB NOT NULL,mime TEXT NOT NULL,size INTEGER NOT NULL,created_at TEXT DEFAULT CURRENT_TIMESTAMP NOT NULL,deleted_at TEXT);").unwrap();
    db.execute("INSERT INTO meta VALUES (?)", [WORKSPACE])
        .unwrap();
    for (id, bytes) in [
        (WORKSPACE, include_bytes!("../fixtures/root.bin").as_slice()),
        (
            "fixture-page",
            include_bytes!("../fixtures/page.bin").as_slice(),
        ),
        (
            "fixture-trash",
            include_bytes!("../fixtures/page.bin").as_slice(),
        ),
        ("orphan", include_bytes!("../fixtures/page.bin").as_slice()),
    ] {
        db.execute(
            "INSERT INTO snapshots(doc_id,data,updated_at) VALUES (?1,?2,?3)",
            params![id, bytes, "2026-01-01 00:00:00.000"],
        )
        .unwrap();
        db.execute(
            "INSERT INTO clocks VALUES (?1,?2)",
            params![id, "2026-01-01 00:00:00.000"],
        )
        .unwrap();
    }
    db.execute(
        "INSERT INTO updates VALUES (?1,?2,?3)",
        params![
            "fixture-page",
            include_bytes!("../fixtures/page-update.bin").as_slice(),
            "2026-01-01 00:00:00.001"
        ],
    )
    .unwrap();
    drop(db);
    (dir, Storage::new(data, true))
}

pub fn db_path(storage: &Storage) -> PathBuf {
    storage
        .data_dir
        .join("workspaces/local")
        .join(WORKSPACE)
        .join("storage.db")
}
