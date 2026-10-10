use crate::{
    Client,
    common::{WORKSPACE, db_path},
};
use affine_local_mcp::{documents, storage::Storage};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use yrs::{Doc, Map, Out, ReadTxn, Transact, Update, updates::decoder::Decode};

fn property(canvas: &Value, collection: &str, id: &str) -> Value {
    canvas[collection]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .unwrap()["properties"]
        .clone()
}
fn counts(db: &Connection) -> Vec<i64> {
    ["updates", "snapshots", "clocks", "blobs"]
        .iter()
        .map(|table| {
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        })
        .collect()
}
fn set_frame_lock(db: &Connection, id: &str, locked: bool) {
    let doc = Doc::new();
    let snapshot: Option<Vec<u8>> = db
        .query_row("SELECT data FROM snapshots WHERE doc_id=?", [id], |r| {
            r.get(0)
        })
        .optional()
        .unwrap();
    if let Some(bytes) = snapshot {
        doc.transact_mut()
            .apply_update(Update::decode_v1(&bytes).unwrap())
            .unwrap();
    }
    let bytes: Vec<Vec<u8>> = db
        .prepare("SELECT data FROM updates WHERE doc_id=? ORDER BY created_at")
        .unwrap()
        .query_map([id], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for bytes in bytes {
        doc.transact_mut()
            .apply_update(Update::decode_v1(&bytes).unwrap())
            .unwrap();
    }
    let before = doc.transact().state_vector();
    {
        let mut txn = doc.transact_mut();
        let Some(Out::YMap(frame)) = txn.get_map("blocks").unwrap().get(&txn, "frame") else {
            panic!()
        };
        frame.insert(&mut txn, "prop:lockedBySelf", locked);
    }
    let clock: String = db
        .query_row("SELECT timestamp FROM clocks WHERE doc_id=?", [id], |r| {
            r.get(0)
        })
        .unwrap();
    let clock = chrono::NaiveDateTime::parse_from_str(&clock, "%Y-%m-%d %H:%M:%S%.f").unwrap();
    let timestamp = chrono::Utc::now()
        .naive_utc()
        .max(clock + chrono::Duration::milliseconds(1))
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string();
    db.execute(
        "INSERT INTO updates(doc_id,data,created_at) VALUES (?1,?2,?3)",
        params![
            id,
            doc.transact().encode_state_as_update_v1(&before),
            timestamp
        ],
    )
    .unwrap();
    db.execute(
        "UPDATE clocks SET timestamp=?1 WHERE doc_id=?2",
        params![timestamp, id],
    )
    .unwrap();
    assert_eq!(
        documents::root_json(&doc, "blocks").unwrap()["frame"]["prop:lockedBySelf"],
        locked
    );
}

pub fn run(client: &mut Client, storage: &Storage) {
    let import = storage.data_dir.join("mcp-imports");
    std::fs::create_dir_all(&import).unwrap();
    std::fs::write(
        import.join("sample.png"),
        include_bytes!("../fixtures/sample-media.png"),
    )
    .unwrap();
    std::fs::write(
        import.join("sample.txt"),
        include_bytes!("../fixtures/sample-attachment.txt"),
    )
    .unwrap();
    std::fs::write(
        import.join("rollback.txt"),
        b"A distinct blob which must roll back",
    )
    .unwrap();
    let id = "mcp-drawing";
    let read_args = json!({"workspaceId":WORKSPACE,"documentId":id});
    client.success("affine_create_document",json!({"workspaceId":WORKSPACE,"documentId":id,"title":"MCP drawing and media","mode":"edgeless","content":""}));
    let strokes = json!({"workspaceId":WORKSPACE,"documentId":id,"strokes":[
        {"id":"ink","points":[[100,100,0.2],[300,300,0.9]],"lineWidth":8,"color":"#ff0000"},
        {"id":"keep","points":[[900,100],[900,300]],"lineWidth":4}
    ]});
    let pen = client.success("affine_draw_pen", strokes.clone());
    assert_eq!(pen["upsertedIds"], json!(["ink", "keep"]));
    assert_eq!(pen["elementCount"], 2);
    assert!(std::path::Path::new(pen["backupPath"].as_str().unwrap()).is_file());
    client.success("affine_draw_pen", strokes);
    let canvas = client.success("affine_read_canvas", read_args.clone());
    assert_eq!(canvas["elementCount"], 2);
    let ink = property(&canvas, "elements", "ink");
    assert_eq!(ink["type"], "brush");
    assert_eq!(ink["points"], json!([[4, 4, 0.2], [204, 204, 0.9]]));
    assert_eq!(ink["color"], "#ff0000");

    let edits = json!({"workspaceId":WORKSPACE,"documentId":id,
        "curves":[{"id":"curve","source":[400,100],"target":[600,300],"label":"Native curve 🌏","endArrow":true}],
        "links":[
            {"id":"web","target":{"kind":"external","url":"https://affine.pro/"},"title":"AFFiNE","position":[100,500]},
            {"id":"local-link","target":{"kind":"document","documentId":"fixture-page"},"title":"Existing local page","position":[650,500]}
        ],
        "media":[
            {"id":"picture","filePath":"sample.png","position":[100,350],"size":[80,80],"caption":"MCP image"},
            {"id":"file","filePath":"sample.txt","position":[650,350],"caption":"MCP attachment"}
        ],
        "frames":[{"id":"frame","title":"Native frame 🌏","bounds":[50,50,1200,700],
            "childIds":["ink","keep","curve","web","local-link","picture","file"],"presentationIndex":0,
            "background":{"light":"#eef2ff","dark":"#1e293b"}}]
    });
    let result = client.success("affine_edit_canvas", edits);
    assert_eq!(result["importedMedia"].as_array().unwrap().len(), 2);
    assert_eq!(
        result["importedMedia"][0]["sourceId"],
        "WSDpuCmfHg_JdoyVwWVLD_uX1o7jfM1xmB9kgtLxmjs"
    );
    assert_eq!(result["importedMedia"][0]["kind"], "image");
    assert_eq!(result["importedMedia"][1]["kind"], "attachment");
    let canvas = client.success("affine_read_canvas", read_args.clone());
    let curve = property(&canvas, "elements", "curve");
    assert_eq!(curve["mode"], 2);
    assert_eq!(curve["source"], json!({"position":[400,100]}));
    assert_eq!(curve["target"], json!({"position":[600,300]}));
    assert_eq!(curve["text"], "Native curve 🌏");
    assert_eq!(curve["rearEndpointStyle"], "Arrow");
    let frame = property(&canvas, "blocks", "frame");
    assert_eq!(frame["sys:flavour"], "affine:frame");
    assert_eq!(frame["prop:title"], "Native frame 🌏");
    assert_eq!(frame["prop:background.light"], "#eef2ff");
    assert_eq!(frame["prop:childElementIds.ink"], true);
    assert_eq!(
        property(&canvas, "blocks", "web")["prop:url"],
        "https://affine.pro/"
    );
    assert_eq!(
        property(&canvas, "blocks", "local-link")["prop:pageId"],
        "fixture-page"
    );
    let picture = property(&canvas, "blocks", "picture");
    assert_eq!(picture["sys:flavour"], "affine:image");
    assert_eq!(picture["prop:width"], 8);
    assert_eq!(picture["prop:height"], 8);
    assert_eq!(picture["prop:xywh"], "[100.0,350.0,80.0,80.0]");
    let file = property(&canvas, "blocks", "file");
    assert_eq!(file["sys:flavour"], "affine:attachment");
    assert_eq!(file["prop:type"], "text/plain");
    let db = Connection::open(db_path(storage)).unwrap();
    let image_key = picture["prop:sourceId"].as_str().unwrap();
    let stored: (Vec<u8>, String) = db
        .query_row(
            "SELECT data,mime FROM blobs WHERE key=? AND deleted_at IS NULL",
            [image_key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored.0, include_bytes!("../fixtures/sample-media.png"));
    assert_eq!(stored.1, "image/png");
    let bytes: Vec<u8> = db
        .query_row(
            "SELECT data FROM blobs WHERE key=?",
            [file["prop:sourceId"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bytes, include_bytes!("../fixtures/sample-attachment.txt"));

    // Media retry and shared references do not duplicate the binary blob.
    let media = json!({"workspaceId":WORKSPACE,"documentId":id,"media":[{"id":"picture-copy","filePath":"sample.png","position":[1300,100]}]});
    client.success("affine_import_media", media.clone());
    client.success("affine_import_media", media);
    assert_eq!(
        db.query_row("SELECT count(*) FROM blobs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    // Updating a frame title/bounds without childIds retains its membership.
    client.success("affine_edit_canvas",json!({"workspaceId":WORKSPACE,"documentId":id,"frames":[{"id":"frame","title":"Renamed frame","bounds":[50,50,1200,700]}]}));
    let current = client.success("affine_read_canvas", read_args.clone());
    assert_eq!(
        property(&current, "blocks", "frame")["prop:childElementIds.ink"],
        true
    );

    // A late frame failure must roll back the already inserted blob and stroke.
    let before_counts = counts(&db);
    // Also force failure AFTER the document update and its clock are written,
    // at the final registry update. The imported blob must roll back too.
    let clock_before: String = db
        .query_row("SELECT timestamp FROM clocks WHERE doc_id=?", [id], |r| {
            r.get(0)
        })
        .unwrap();
    db.execute_batch(&format!("CREATE TRIGGER fail_media_registry BEFORE INSERT ON updates WHEN NEW.doc_id='{WORKSPACE}' BEGIN SELECT RAISE(ABORT,'forced media registry failure'); END;")).unwrap();
    let failed = client.call(
        "affine_edit_canvas",
        json!({"workspaceId":WORKSPACE,"documentId":id,
        "strokes":[{"id":"late-rollback-ink","points":[[0,0],[10,10]]}],
        "media":[{"id":"late-rollback-file","filePath":"rollback.txt","position":[0,0]}]}),
    );
    db.execute_batch("DROP TRIGGER fail_media_registry")
        .unwrap();
    assert_eq!(failed["isError"], true);
    assert!(
        failed["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("forced media registry failure")
    );
    assert_eq!(counts(&db), before_counts);
    assert_eq!(
        db.query_row("SELECT timestamp FROM clocks WHERE doc_id=?", [id], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        clock_before
    );
    assert_eq!(
        client.success("affine_read_canvas", read_args.clone()),
        current
    );
    let failed = client.call("affine_edit_canvas",json!({"workspaceId":WORKSPACE,"documentId":id,
        "strokes":[{"id":"rollback-ink","points":[[0,0],[10,10]]}],
        "media":[{"id":"rollback-file","filePath":"rollback.txt","position":[0,0]}],
        "frames":[{"id":"bad-frame","title":"Invalid child","bounds":[0,0,500,500],"childIds":["missing"]}]}));
    assert_eq!(failed["isError"], true);
    assert_eq!(counts(&db), before_counts);
    assert_eq!(
        client.success("affine_read_canvas", read_args.clone()),
        current
    );
    for bad in [
        json!({"frames":[{"id":"f1","title":"Cycle one","bounds":[0,0,500,500],"childIds":["f2"]},{"id":"f2","title":"Cycle two","bounds":[0,0,500,500],"childIds":["f1"]}]}),
        json!({"frames":[{"id":"duplicate-frame","title":"Duplicate owner","bounds":[0,0,500,500],"childIds":["ink"]}]}),
        json!({"links":[{"id":"missing-link","position":[0,0],"target":{"kind":"document","documentId":"orphan"}}]}),
        json!({"media":[{"id":"escaped","filePath":"../workspaces/local/fixture-workspace/storage.db","position":[0,0]}]}),
        json!({"deleteIds":["mcp-drawing-surface"],"confirmDelete":true}),
    ] {
        let mut bad = bad;
        bad["workspaceId"] = json!(WORKSPACE);
        bad["documentId"] = json!(id);
        assert_eq!(client.call("affine_edit_canvas", bad)["isError"], true);
        assert_eq!(counts(&db), before_counts);
        assert_eq!(
            client.success("affine_read_canvas", read_args.clone()),
            current
        );
    }

    // Locks inherited from a frame protect pen strokes, including swept erasing.
    set_frame_lock(&db, id, true);
    let explicit =
        json!({"workspaceId":WORKSPACE,"documentId":id,"strokeIds":["ink"],"confirm":true});
    assert_eq!(
        client.call("affine_erase_canvas", explicit)["isError"],
        true
    );
    let path = json!({"workspaceId":WORKSPACE,"documentId":id,"path":[[250,0],[250,600]],"radius":4,"confirm":true});
    let skipped = client.success("affine_erase_canvas", path.clone());
    assert_eq!(skipped["deletedIds"], json!([]));
    assert_eq!(skipped["skippedLockedIds"], json!(["ink"]));
    assert_eq!(client.call("affine_draw_pen",json!({"workspaceId":WORKSPACE,"documentId":id,"strokes":[{"id":"ink","points":[[0,0],[10,10]]}]}))["isError"],true);
    set_frame_lock(&db, id, false);
    assert_eq!(
        client.call(
            "affine_erase_canvas",
            json!({"workspaceId":WORKSPACE,"documentId":id,"strokeIds":["ink"]})
        )["isError"],
        true
    );
    let erased = client.success("affine_erase_canvas", path.clone());
    assert_eq!(erased["deletedIds"], json!(["ink"]));
    assert_eq!(
        client.success("affine_erase_canvas", path)["deletedIds"],
        json!([])
    );
    let canvas = client.success("affine_read_canvas", read_args.clone());
    assert!(
        canvas["elements"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["id"] != "ink")
    );
    assert_eq!(property(&canvas, "elements", "keep")["type"], "brush");
    assert!(property(&canvas, "blocks", "frame")["prop:childElementIds.ink"].is_null());
    assert_eq!(
        property(&canvas, "blocks", "frame")["prop:childElementIds.keep"],
        true
    );

    // Exercise the exact documented combined example through MCP, not just parse it.
    std::fs::copy(import.join("sample.png"), import.join("sample-media.png")).unwrap();
    std::fs::copy(
        import.join("sample.txt"),
        import.join("sample-attachment.txt"),
    )
    .unwrap();
    let mut example: Value =
        serde_json::from_str(include_str!("../../examples/native-canvas-features.json")).unwrap();
    example["workspaceId"] = json!(WORKSPACE);
    example["documentId"] = json!("mcp-example");
    client.success("affine_create_document",json!({"workspaceId":WORKSPACE,"documentId":"mcp-example","title":"Documented native example","content":"","mode":"edgeless"}));
    client.success("affine_edit_canvas", example);
    let example = client.success(
        "affine_read_canvas",
        json!({"workspaceId":WORKSPACE,"documentId":"mcp-example"}),
    );
    assert_eq!(property(&example, "elements", "demo-pen")["type"], "brush");
    assert_eq!(property(&example, "elements", "demo-curve")["mode"], 2);
    assert_eq!(
        property(&example, "blocks", "demo-frame")["prop:childElementIds.demo-image"],
        true
    );
    assert_eq!(
        property(&example, "blocks", "demo-link")["sys:flavour"],
        "affine:bookmark"
    );
    assert_eq!(
        property(&example, "blocks", "demo-image")["sys:flavour"],
        "affine:image"
    );
    assert_eq!(
        property(&example, "blocks", "demo-file")["sys:flavour"],
        "affine:attachment"
    );

    // Removing the frame preserves its content; media deletion never removes shared blobs.
    client.success(
        "affine_edit_canvas",
        json!({"workspaceId":WORKSPACE,"documentId":id,"deleteIds":["frame"],"confirmDelete":true}),
    );
    let canvas = client.success("affine_read_canvas", read_args.clone());
    assert_eq!(property(&canvas, "elements", "keep")["type"], "brush");
    assert_eq!(
        property(&canvas, "blocks", "picture")["prop:sourceId"],
        image_key
    );
    client.success("affine_edit_canvas",json!({"workspaceId":WORKSPACE,"documentId":id,"deleteIds":["web","local-link","file","curve","picture"],"confirmDelete":true}));
    let canvas = client.success("affine_read_canvas", read_args);
    assert_eq!(canvas["elementCount"], 1);
    assert_eq!(canvas["canvasBlockCount"], 1);
    assert_eq!(
        property(&canvas, "blocks", "picture-copy")["prop:sourceId"],
        image_key
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM blobs WHERE deleted_at IS NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
}
