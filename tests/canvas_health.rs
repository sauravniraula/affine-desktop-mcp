mod common;
use affine_local_mcp::{canvas, storage::build_document};
use common::{WORKSPACE, db_path, fixture};
use rusqlite::{Connection, params};
use serde_json::json;
use yrs::{Map, Out, ReadTxn, Transact};

// Read every column through SQLite so committed WAL writes cannot escape the
// read-only assertion, including clock and blob metadata-only changes.
fn database_state(db: &Connection) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    [
        "SELECT * FROM snapshots ORDER BY doc_id",
        "SELECT * FROM updates ORDER BY doc_id,created_at",
        "SELECT * FROM clocks ORDER BY doc_id",
        "SELECT * FROM blobs ORDER BY key",
    ]
    .into_iter()
    .map(|query| {
        let mut statement = db.prepare(query).unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| {
                (0..columns).map(|column| row.get(column)).collect()
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    })
    .collect()
}

#[test]
fn read_only_state_assertion_detects_wal_only_changes() {
    let (_dir, storage) = fixture();
    let db = Connection::open(db_path(&storage)).unwrap();
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    let observer = Connection::open(db_path(&storage)).unwrap();
    for query in [
        "UPDATE snapshots SET updated_at='wal-only-change' WHERE doc_id='fixture-page'",
        "UPDATE updates SET data=x'01' WHERE doc_id='fixture-page'",
        "UPDATE clocks SET timestamp='wal-only-change' WHERE doc_id='fixture-page'",
        "INSERT INTO blobs(key,data,mime,size) VALUES ('wal-only',x'01','image/png',1)",
    ] {
        let before = database_state(&observer);
        let main_before = std::fs::read(db_path(&storage)).unwrap();
        db.execute_batch(query).unwrap();
        assert_eq!(std::fs::read(db_path(&storage)).unwrap(), main_before);
        assert!(
            database_state(&observer) != before,
            "The read-only assertion must detect committed WAL changes: {query}"
        );
    }
}

#[test]
fn read_canvas_reports_legacy_identity_defects_without_claiming_ui_verification() {
    let (_dir, storage) = fixture();
    let doc = build_document("fixture-page", "Legacy drawing", "", "edgeless").unwrap();
    let elements = canvas::elements(&doc).unwrap();
    for (id, xywh) in [
        ("missing", "[100,200,50,80]"),
        ("mismatch", "[-20,10,40,30]"),
    ] {
        canvas::upsert(&doc, &elements, id, json!({"type":"shape","xywh":xywh})).unwrap();
        let mut txn = doc.transact_mut();
        let Some(Out::YMap(map)) = elements.get(&txn, id) else {
            panic!()
        };
        if id == "missing" {
            map.remove(&mut txn, "id");
        } else {
            map.insert(&mut txn, "id", "wrong");
        }
    }
    let db = Connection::open(db_path(&storage)).unwrap();
    db.execute("DELETE FROM updates WHERE doc_id='fixture-page'", [])
        .unwrap();
    db.execute(
        "UPDATE snapshots SET data=? WHERE doc_id='fixture-page'",
        params![
            doc.transact()
                .encode_state_as_update_v1(&Default::default())
        ],
    )
    .unwrap();
    let before = std::fs::read(db_path(&storage)).unwrap();
    let logical_before = database_state(&db);
    let read = storage.read_canvas(WORKSPACE, "fixture-page").unwrap();
    assert_eq!(read["validation"]["idIntegrityValid"], false);
    assert_eq!(read["validation"]["uiVerified"], false);
    let issues = read["validation"]["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 2);
    assert!(
        issues
            .iter()
            .any(|v| v["code"] == "missing-element-id" && v["elementId"] == "missing")
    );
    assert!(
        issues
            .iter()
            .any(|v| v["code"] == "conflicting-element-id" && v["elementId"] == "mismatch")
    );
    assert_eq!(read["geometryBounds"], json!([-20.0, 10.0, 170.0, 270.0]));
    assert_eq!(read["mode"], "page"); // Registry primary mode, not inferred from canvas contents.
    assert!(read["viewHint"].as_str().unwrap().contains("Edgeless"));
    assert_eq!(std::fs::read(db_path(&storage)).unwrap(), before);
    assert_eq!(database_state(&db), logical_before);
}
