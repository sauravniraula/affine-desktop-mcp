use crate::{
    Client,
    common::{WORKSPACE, db_path},
};
use affine_local_mcp::storage::Storage;
use rusqlite::Connection;
use serde_json::{Value, json};

fn element(canvas: &Value, id: &str) -> Value {
    canvas["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == id)
        .unwrap()["properties"]
        .clone()
}

pub fn run(client: &mut Client, storage: &Storage) {
    let args = json!({
        "workspaceId":WORKSPACE,"documentId":"native-diagram","title":"Native flowchart",
        "nodes":[
            {"id":"start","text":"Start 🌏","shape":"ellipse"},
            {"id":"decision","text":"Valid?","shape":"diamond"},
            {"id":"process","text":"Save\nYjs update","shape":"roundedRect"},
            {"id":"end","text":"Done","shape":"rect"},
            {"id":"error","text":"Stop","shape":"triangle","style":"scribbled"}
        ],
        "edges":[
            {"id":"e1","source":"start","target":"decision"},
            {"id":"e2","source":"decision","target":"process","text":"Yes","routing":"straight"},
            {"id":"e3","source":"process","target":"end","routing":"curve"},
            {"id":"e4","source":"decision","target":"error","text":"No"}
        ]
    });
    let created = client.success("affine_create_diagram", args.clone());
    assert_eq!(created["alreadyExists"], false);
    assert!(std::path::Path::new(created["backupPath"].as_str().unwrap()).is_file());
    let read_args = json!({"workspaceId":WORKSPACE,"documentId":"native-diagram"});
    let canvas = client.success("affine_read_canvas", read_args.clone());
    assert_eq!(canvas["elementCount"], 9);
    assert_eq!(element(&canvas, "start")["shapeType"], "ellipse");
    assert_eq!(element(&canvas, "decision")["shapeType"], "diamond");
    assert_eq!(element(&canvas, "process")["shapeType"], "rect");
    assert_eq!(element(&canvas, "process")["radius"], 0.1);
    assert_eq!(element(&canvas, "end")["shapeType"], "rect");
    assert_eq!(element(&canvas, "error")["shapeType"], "triangle");
    assert_eq!(element(&canvas, "error")["shapeStyle"], "Scribbled");
    assert_eq!(element(&canvas, "e2")["mode"], 0);
    assert_eq!(element(&canvas, "e3")["mode"], 2);
    assert_eq!(element(&canvas, "e4")["mode"], 1);
    assert_eq!(element(&canvas, "e1")["source"]["id"], "start");
    assert_eq!(
        element(&canvas, "e1")["target"]["position"],
        json!([0.5, 0])
    );
    assert_eq!(element(&canvas, "e1")["rearEndpointStyle"], "Arrow");
    let text = client.success("affine_read_document", read_args.clone());
    assert_eq!(text["mode"], "edgeless");
    assert_eq!(
        text["blockCount"], 2,
        "Must be page + surface, not a text note"
    );
    assert!(
        text["markdown"]
            .as_str()
            .unwrap()
            .contains("[Canvas shape start] Start 🌏")
    );
    assert_eq!(
        client.success("affine_create_diagram", args.clone())["alreadyExists"],
        true
    );
    let mut conflict = args.clone();
    conflict["nodes"][0]["text"] = json!("Changed diagram");
    assert_eq!(
        client.call("affine_create_diagram", conflict)["isError"],
        true
    );
    let mut invalid = args;
    invalid["documentId"] = json!("invalid-diagram");
    invalid["edges"][0]["source"] = json!("missing");
    assert_eq!(
        client.call("affine_create_diagram", invalid)["isError"],
        true
    );
    assert_eq!(
        client.call(
            "affine_read_canvas",
            json!({"workspaceId":WORKSPACE,"documentId":"invalid-diagram"})
        )["isError"],
        true
    );

    // Native labels/geometry are updated, but unrelated shapes are preserved.
    let edit = json!({"workspaceId":WORKSPACE,"documentId":"native-diagram",
        "nodes":[{"id":"start","text":"Updated 🌏","shape":"ellipse","position":[-120,50],"fillColor":{"light":"#ffffff","dark":"#222222"}}],
        "edges":[{"id":"e1","source":"start","target":"decision","text":"Go","sourceAnchor":"right","targetAnchor":"left","endEndpoint":"circle"}]
    });
    client.success("affine_edit_canvas", edit.clone());
    let edited = client.success("affine_read_canvas", read_args.clone());
    assert_eq!(edited["elementCount"], 9);
    assert_eq!(element(&edited, "start")["text"], "Updated 🌏");
    assert_eq!(
        element(&edited, "start")["xywh"],
        "[-120.0,50.0,260.0,120.0]"
    );
    assert_eq!(
        element(&edited, "e1")["source"]["position"],
        json!([1, 0.5])
    );
    assert_eq!(element(&edited, "e1")["rearEndpointStyle"], "Circle");
    assert_eq!(element(&edited, "end"), element(&canvas, "end"));
    client.success("affine_edit_canvas", edit.clone());
    assert_eq!(
        client.success("affine_read_canvas", read_args.clone()),
        edited
    );

    let db = Connection::open(db_path(storage)).unwrap();
    let count_updates = || {
        db.query_row("SELECT count(*) FROM updates", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    };
    let before = count_updates();
    db.execute_batch("CREATE TRIGGER fail_canvas_registry BEFORE INSERT ON updates WHEN NEW.doc_id='test-workspace' BEGIN SELECT RAISE(ABORT,'canvas rollback'); END;").unwrap();
    let mut failed_edit = edit.clone();
    failed_edit["nodes"][0]["text"] = json!("Must not persist");
    assert_eq!(
        client.call("affine_edit_canvas", failed_edit)["isError"],
        true
    );
    assert_eq!(count_updates(), before);
    assert_eq!(
        client.success("affine_read_canvas", read_args.clone()),
        edited
    );
    db.execute_batch("DROP TRIGGER fail_canvas_registry")
        .unwrap();
    let invalid_edit = json!({"workspaceId":WORKSPACE,"documentId":"native-diagram",
    "nodes":[
        {"id":"start","text":"Must roll back with next node","shape":"ellipse"},
        {"id":"e2","text":"Type conflict","position":[10,20]}
    ]});
    assert_eq!(
        client.call("affine_edit_canvas", invalid_edit)["isError"],
        true
    );
    assert_eq!(count_updates(), before);
    assert_eq!(
        client.success("affine_read_canvas", read_args.clone()),
        edited
    );
    let mut dangling = edit;
    dangling["edges"][0]["target"] = json!("orphan-shape");
    assert_eq!(client.call("affine_edit_canvas", dangling)["isError"], true);
    assert_eq!(count_updates(), before);

    let delete =
        json!({"workspaceId":WORKSPACE,"documentId":"native-diagram","deleteIds":["decision"]});
    assert_eq!(
        client.call("affine_edit_canvas", delete.clone())["isError"],
        true
    );
    assert_eq!(count_updates(), before);
    let mut delete = delete;
    delete["confirmDelete"] = json!(true);
    let result = client.success("affine_edit_canvas", delete.clone());
    assert_eq!(result["deletedIds"], json!(["decision", "e1", "e2", "e4"]));
    assert_eq!(result["elementCount"], 5);
    client.success("affine_edit_canvas", delete);
    assert_eq!(
        client.success("affine_read_canvas", read_args.clone())["elementCount"],
        5
    );

    // Adding shapes to a note canvas must preserve its paragraph and other blocks.
    let note_edit = json!({"workspaceId":WORKSPACE,"documentId":"mcp-edgeless",
        "nodes":[{"id":"native-box","text":"Native shape","position":[1000,100]}]});
    client.success("affine_edit_canvas", note_edit.clone());
    let note = client.success(
        "affine_read_document",
        json!({"workspaceId":WORKSPACE,"documentId":"mcp-edgeless"}),
    );
    assert!(
        note["markdown"]
            .as_str()
            .unwrap()
            .contains("Created through actual MCP stdio 🌏")
    );
    assert!(
        note["markdown"]
            .as_str()
            .unwrap()
            .contains("[Canvas shape native-box] Native shape")
    );
    assert_eq!(note["blockCount"], 4);

    let mut read_only = Client::start(&storage.data_dir, false);
    assert_eq!(
        read_only.call("affine_edit_canvas", note_edit)["isError"],
        true
    );
    assert_eq!(read_only.call("affine_create_diagram", json!({"workspaceId":WORKSPACE,"title":"Disabled","nodes":[{"id":"x","text":"No write"}]}))["isError"], true);
    assert_eq!(
        read_only.success("affine_read_canvas", read_args)["elementCount"],
        5
    );
}
