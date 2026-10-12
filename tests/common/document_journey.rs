use crate::{
    Client,
    common::{WORKSPACE, db_path},
};
use affine_local_mcp::storage::Storage;
use rusqlite::Connection;
use serde_json::{Value, json};
use yrs::{Doc, Map, Out, ReadTxn, SharedRef, Transact, Update, updates::decoder::Decode};

pub(super) fn stored_doc(db: &Connection, id: &str) -> Doc {
    let doc = Doc::new();
    for query in [
        "SELECT data FROM snapshots WHERE doc_id=?1",
        "SELECT data FROM updates WHERE doc_id=?1 ORDER BY created_at",
    ] {
        let mut stmt = db.prepare(query).unwrap();
        for bytes in stmt
            .query_map([id], |row| row.get::<_, Vec<u8>>(0))
            .unwrap()
        {
            doc.transact_mut()
                .apply_update(Update::decode_v1(&bytes.unwrap()).unwrap())
                .unwrap();
        }
    }
    doc
}

// Compare actual persistent rows, not just counts, to catch partial updates and clocks.
type StoredRow = (String, Vec<u8>, String);

pub(super) fn database_state(db: &Connection) -> Vec<Vec<StoredRow>> {
    [
        "SELECT doc_id,data,updated_at FROM snapshots ORDER BY doc_id",
        "SELECT doc_id,data,created_at FROM updates ORDER BY doc_id,created_at",
        "SELECT doc_id,CAST(timestamp AS BLOB),timestamp FROM clocks ORDER BY doc_id",
        "SELECT key,data,mime FROM blobs ORDER BY key",
    ]
    .into_iter()
    .map(|query| {
        db.prepare(query)
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    })
    .collect()
}

pub fn run(client: &mut Client, storage: &Storage) {
    let args = json!({"workspaceId":WORKSPACE,"documentId":"managed-page"});
    client.success("affine_create_document", json!({"workspaceId":WORKSPACE,"documentId":"managed-page","title":"Original 🌏","content":"Preserved management body","mode":"edgeless"}));
    let db = Connection::open(db_path(storage)).unwrap();
    let original_canvas = client.success("affine_read_canvas", args.clone());
    let original = stored_doc(&db, "managed-page");
    let original_title_id = {
        let txn = original.transact();
        let blocks = txn.get_map("blocks").unwrap();
        let Out::YMap(page) = blocks.get(&txn, "managed-page-page").unwrap() else {
            panic!()
        };
        let Out::YText(title) = page.get(&txn, "prop:title").unwrap() else {
            panic!()
        };
        title.hook().id().clone()
    };
    let updated = client.success("affine_update_document", json!({"workspaceId":WORKSPACE,"documentId":"managed-page","title":"  Renamed 日本語 🌏  ","mode":"page"}));
    assert_eq!(updated["changed"], true);
    assert_eq!(updated["reopenAffine"], true);
    let backup = Connection::open(updated["backupPath"].as_str().unwrap()).unwrap();
    assert_eq!(
        backup
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        affine_local_mcp::documents::render(&stored_doc(&backup, "managed-page"))
            .unwrap()
            .title,
        "Original 🌏"
    );
    let read = client.success("affine_read_document", args.clone());
    assert_eq!(read["title"], "Renamed 日本語 🌏");
    assert_eq!(read["mode"], "page");
    assert_eq!(
        read["markdown"],
        "# Renamed 日本語 🌏\n\nPreserved management body"
    );
    assert_eq!(
        client.success("affine_read_canvas", args.clone()),
        original_canvas
    );
    let saved = stored_doc(&db, "managed-page");
    let txn = saved.transact();
    let blocks = txn.get_map("blocks").unwrap();
    let Out::YMap(page) = blocks.get(&txn, "managed-page-page").unwrap() else {
        panic!()
    };
    let Out::YText(title) = page.get(&txn, "prop:title").unwrap() else {
        panic!()
    };
    assert_eq!(title.hook().id(), &original_title_id);
    drop(txn);
    let list = client.success("affine_list_documents", json!({"workspaceId":WORKSPACE}));
    let entry = list["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "managed-page")
        .unwrap();
    assert_eq!(entry["title"], read["title"]);
    assert_eq!(entry["mode"], "page");
    let before = database_state(&db);
    assert_eq!(client.success("affine_update_document", json!({"workspaceId":WORKSPACE,"documentId":"managed-page","title":"Renamed 日本語 🌏","mode":"page"}))["changed"], false);
    assert_eq!(database_state(&db), before);
    // Omitted fields stay unchanged, and mode-only updates do not touch the page body.
    client.success(
        "affine_update_document",
        json!({"workspaceId":WORKSPACE,"documentId":"managed-page","mode":"edgeless"}),
    );
    client.success(
        "affine_update_document",
        json!({"workspaceId":WORKSPACE,"documentId":"managed-page","title":"Final title"}),
    );
    assert_eq!(
        client.success("affine_read_document", args.clone())["mode"],
        "edgeless"
    );

    let before = database_state(&db);
    assert_eq!(
        client.call(
            "affine_delete_document",
            json!({"workspaceId":WORKSPACE,"documentId":"managed-page","confirm":false})
        )["isError"],
        true
    );
    assert_eq!(database_state(&db), before);
    let deleted = client.success(
        "affine_delete_document",
        json!({"workspaceId":WORKSPACE,"documentId":"managed-page","confirm":true}),
    );
    assert_eq!(deleted["trash"], true);
    assert_eq!(deleted["changed"], true);
    assert!(deleted["trashedAt"].is_number());
    assert_eq!(
        client.success(
            "affine_search_documents",
            json!({"workspaceId":WORKSPACE,"query":"Preserved management body"})
        )["total"],
        0
    );
    for tool in ["affine_read_document", "affine_read_canvas"] {
        assert_eq!(client.call(tool, args.clone())["isError"], true);
    }
    assert_eq!(
        client.call(
            "affine_update_document",
            json!({"workspaceId":WORKSPACE,"documentId":"managed-page","title":"Not allowed"})
        )["isError"],
        true
    );
    assert_eq!(client.call("affine_edit_canvas", json!({"workspaceId":WORKSPACE,"documentId":"managed-page","nodes":[{"id":"new","text":"Blocked","position":[0,0]}]}))["isError"], true);
    let trash = client.success(
        "affine_list_trash",
        json!({"workspaceId":WORKSPACE,"limit":1}),
    );
    assert_eq!(trash["total"], 2);
    assert_eq!(trash["hasMore"], true);
    let next = client.success(
        "affine_list_trash",
        json!({"workspaceId":WORKSPACE,"offset":trash["nextOffset"],"limit":1}),
    );
    assert_eq!(next["documents"][0]["id"], "managed-page");
    assert_eq!(next["documents"][0]["trashedAt"], deleted["trashedAt"]);
    assert_eq!(next["hasMore"], false);
    let before = database_state(&db);
    assert_eq!(
        client.success(
            "affine_delete_document",
            json!({"workspaceId":WORKSPACE,"documentId":"managed-page","confirm":true})
        )["changed"],
        false
    );
    assert_eq!(database_state(&db), before);
    let restored = client.success("affine_restore_document", args.clone());
    assert_eq!(restored["trash"], false);
    assert_eq!(restored["trashedAt"], Value::Null);
    assert_eq!(
        client.success("affine_read_document", args.clone())["markdown"],
        "# Final title\n\nPreserved management body"
    );
    assert_eq!(
        client.success("affine_read_canvas", args.clone()),
        original_canvas
    );
    assert_eq!(
        client.success(
            "affine_search_documents",
            json!({"workspaceId":WORKSPACE,"query":"Preserved management body"})
        )["total"],
        1
    );
    assert_eq!(
        client.success("affine_list_trash", json!({"workspaceId":WORKSPACE}))["total"],
        1
    );
    let before = database_state(&db);
    assert_eq!(
        client.success("affine_restore_document", args.clone())["changed"],
        false
    );
    assert_eq!(database_state(&db), before);

    // Reject invalid, unregistered and internal IDs; permissions apply to every mutation.
    for id in [
        "orphan",
        WORKSPACE,
        "db$docProperties",
        "../fixture-page",
        "missing",
    ] {
        for (tool, extra) in [
            ("affine_update_document", json!({"title":"Blocked"})),
            ("affine_delete_document", json!({"confirm":true})),
            ("affine_restore_document", json!({})),
        ] {
            let mut args = extra;
            args["workspaceId"] = json!(WORKSPACE);
            args["documentId"] = json!(id);
            assert_eq!(client.call(tool, args)["isError"], true);
        }
    }
    let mut read_only = Client::start(&storage.data_dir, false);
    for (tool, extra) in [
        ("affine_update_document", json!({"title":"Blocked"})),
        ("affine_delete_document", json!({"confirm":true})),
        ("affine_restore_document", json!({})),
    ] {
        let mut args = extra;
        args["workspaceId"] = json!(WORKSPACE);
        args["documentId"] = json!("managed-page");
        assert!(
            read_only.call(tool, args)["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains("Writes disabled")
        );
    }
    assert_eq!(
        read_only.success("affine_list_trash", json!({"workspaceId":WORKSPACE}))["total"],
        1
    );
    assert_eq!(database_state(&db), before);

    // Fail the final registry write: title, mode and all clocks roll back together.
    db.execute_batch("CREATE TRIGGER fail_management BEFORE INSERT ON updates WHEN NEW.doc_id='test-workspace' BEGIN SELECT RAISE(ABORT,'injected management failure'); END;").unwrap();
    for (tool, extra) in [
        (
            "affine_update_document",
            json!({"title":"Rollback title","mode":"page"}),
        ),
        ("affine_delete_document", json!({"confirm":true})),
    ] {
        let mut args = extra;
        args["workspaceId"] = json!(WORKSPACE);
        args["documentId"] = json!("managed-page");
        assert_eq!(client.call(tool, args)["isError"], true);
        assert_eq!(database_state(&db), before);
    }
    db.execute_batch("DROP TRIGGER fail_management").unwrap();
    client.success("affine_update_document", json!({"workspaceId":WORKSPACE,"documentId":"managed-page","title":"Retry title","mode":"page"}));
    client.success(
        "affine_delete_document",
        json!({"workspaceId":WORKSPACE,"documentId":"managed-page","confirm":true}),
    );
    let before = database_state(&db);
    db.execute_batch("CREATE TRIGGER fail_restore BEFORE INSERT ON updates WHEN NEW.doc_id='test-workspace' BEGIN SELECT RAISE(ABORT,'injected restore failure'); END;").unwrap();
    assert_eq!(
        client.call("affine_restore_document", args.clone())["isError"],
        true
    );
    assert_eq!(database_state(&db), before);
    db.execute_batch("DROP TRIGGER fail_restore").unwrap();
    client.success("affine_restore_document", args);
    assert_eq!(
        client.success(
            "affine_read_document",
            json!({"workspaceId":WORKSPACE,"documentId":"managed-page"})
        )["title"],
        "Retry title"
    );
}
