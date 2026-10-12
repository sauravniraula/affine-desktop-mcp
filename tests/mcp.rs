#[path = "common/canvas_journey.rs"]
mod canvas_journey;
mod common;
#[path = "common/document_journey.rs"]
mod document_journey;
#[path = "common/drawing_journey.rs"]
mod drawing_journey;
#[path = "common/organization_journey.rs"]
mod organization_journey;
#[path = "common/workspace_journey.rs"]
mod workspace_journey;
use common::{WORKSPACE, db_path, fixture};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

struct Client {
    child: Child,
    stdin: ChildStdin,
    responses: Receiver<Value>,
    id: u64,
}
impl Client {
    fn start(data_dir: &std::path::Path, writes: bool) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_affine-local-mcp"));
        command
            .arg("--data-dir")
            .arg(data_dir)
            .env_remove("AFFINE_WORKSPACE_IDS")
            .env_remove("AFFINE_ENABLE_WRITES");
        if writes {
            command.arg("--enable-writes");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, responses) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let value: Value = serde_json::from_str(&line.unwrap())
                    .expect("stdout must contain only JSON MCP messages");
                if sender.send(value).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            stdin,
            responses,
            id: 0,
        };
        let init = client.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"rust-integration-test","version":"1"}}));
        assert!(
            init["result"]["capabilities"]["tools"].is_object(),
            "{init}"
        );
        client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        client
    }
    fn send(&mut self, value: Value) {
        writeln!(self.stdin, "{value}").unwrap();
        self.stdin.flush().unwrap();
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        loop {
            let value = self
                .responses
                .recv_timeout(Duration::from_secs(20))
                .expect("MCP request timed out");
            if value["id"] == id {
                return value;
            }
        }
    }
    fn call(&mut self, name: &str, args: Value) -> Value {
        let response = self.request("tools/call", json!({"name":name,"arguments":args}));
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }
    fn success(&mut self, name: &str, args: Value) -> Value {
        let result = self.call(name, args);
        assert_eq!(result["isError"], false, "{result}");
        result["structuredContent"].clone()
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn actual_stdio_mcp_creation_read_search_retry_rollback_and_running_app_guard() {
    let (_dir, storage) = fixture();
    let mut client = Client::start(&storage.data_dir, true);
    let tools = client.request("tools/list", json!({}));
    let tools = tools["result"]["tools"].as_array().unwrap();
    let expected_tools = [
        "affine_list_workspaces",
        "affine_create_workspace",
        "affine_delete_workspace",
        "affine_list_documents",
        "affine_read_document",
        "affine_search_documents",
        "affine_create_document",
        "affine_create_diagram",
        "affine_read_canvas",
        "affine_edit_canvas",
        "affine_draw_pen",
        "affine_erase_canvas",
        "affine_import_media",
        "affine_update_document",
        "affine_delete_document",
        "affine_restore_document",
        "affine_list_trash",
        "affine_list_tags",
        "affine_manage_tag",
        "affine_set_document_tags",
        "affine_list_collections",
        "affine_manage_collection",
        "affine_list_folders",
        "affine_manage_folder",
    ];
    assert_eq!(tools.len(), expected_tools.len());
    for name in expected_tools {
        assert!(tools.iter().any(|tool| tool["name"] == name));
    }
    assert!(
        tools
            .iter()
            .all(|tool| tool["annotations"]["openWorldHint"] == false)
    );
    assert_eq!(
        tools
            .iter()
            .filter(|tool| tool["annotations"]["readOnlyHint"] == false)
            .count(),
        [
            "affine_create_document",
            "affine_create_workspace",
            "affine_delete_workspace",
            "affine_create_diagram",
            "affine_edit_canvas",
            "affine_draw_pen",
            "affine_erase_canvas",
            "affine_import_media",
            "affine_update_document",
            "affine_delete_document",
            "affine_restore_document",
            "affine_manage_tag",
            "affine_set_document_tags",
            "affine_manage_collection",
            "affine_manage_folder"
        ]
        .len()
    );
    let workspaces = client.success("affine_list_workspaces", json!({}));
    assert_eq!(workspaces["workspaces"][0]["id"], WORKSPACE);
    for (id, mode) in [("mcp-page", "page"), ("mcp-edgeless", "edgeless")] {
        let args = json!({"workspaceId":WORKSPACE,"documentId":id,"title":format!("MCP {mode} test"),"content":"Created through actual MCP stdio 🌏","mode":mode});
        let created = client.success("affine_create_document", args.clone());
        assert_eq!(created["alreadyExists"], false);
        let backup = Connection::open(created["backupPath"].as_str().unwrap()).unwrap();
        let integrity: String = backup
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
        let read = client.success(
            "affine_read_document",
            json!({"workspaceId":WORKSPACE,"documentId":id}),
        );
        assert_eq!(read["mode"], mode);
        assert!(
            read["markdown"]
                .as_str()
                .unwrap()
                .contains("Created through actual MCP stdio 🌏")
        );
        assert_eq!(read["blockCount"], 4);
        assert_eq!(
            client.success("affine_create_document", args.clone())["alreadyExists"],
            true
        );
        let mut conflict = args;
        conflict["content"] = json!("different content");
        assert_eq!(
            client.call("affine_create_document", conflict)["isError"],
            true
        );
    }
    let found = client.success(
        "affine_search_documents",
        json!({"workspaceId":WORKSPACE,"query":"ACTUAL MCP STDIO","limit":1}),
    );
    assert_eq!(found["total"], 2);
    assert_eq!(found["results"].as_array().unwrap().len(), 1);
    assert_eq!(found["hasMore"], true);
    let list = client.success(
        "affine_list_documents",
        json!({"workspaceId":WORKSPACE,"limit":2}),
    );
    assert_eq!(list["total"], 3);
    assert_eq!(list["nextOffset"], 2);
    let bad = client.request("tools/call", json!({"name":"affine_read_document","arguments":{"workspaceId":WORKSPACE,"documentId":"mcp-page","maxChars":-1}}));
    assert!(
        bad.get("error").is_some() || bad["result"]["isError"] == true,
        "{bad}"
    );
    assert_eq!(
        client.call(
            "affine_read_document",
            json!({"workspaceId":WORKSPACE,"documentId":"orphan"})
        )["isError"],
        true
    );

    // Inject failure at the last dependent update: document, properties and clocks must roll back.
    let db = Connection::open(db_path(&storage)).unwrap();
    let before: i64 = db
        .query_row("SELECT count(*) FROM updates", [], |row| row.get(0))
        .unwrap();
    db.execute_batch("CREATE TRIGGER fail_registry BEFORE INSERT ON updates WHEN NEW.doc_id='test-workspace' BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    let failed_args = json!({"workspaceId":WORKSPACE,"documentId":"mcp-rollback","title":"Rollback test","content":"Retry succeeds","mode":"page"});
    let failed = client.call("affine_create_document", failed_args.clone());
    assert_eq!(failed["isError"], true);
    assert_eq!(
        db.query_row("SELECT count(*) FROM updates", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        before
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM clocks WHERE doc_id='mcp-rollback'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        client.success("affine_list_documents", json!({"workspaceId":WORKSPACE}))["total"],
        3
    );
    db.execute_batch("DROP TRIGGER fail_registry").unwrap();
    client.success("affine_create_document", failed_args);
    assert_eq!(
        client.success(
            "affine_read_document",
            json!({"workspaceId":WORKSPACE,"documentId":"mcp-rollback"})
        )["markdown"],
        "# Rollback test\n\nRetry succeeds"
    );

    // Concurrent stable-ID retries through the same storage must create exactly one document.
    let a = storage.clone();
    let b = storage.clone();
    let a = std::thread::spawn(move || {
        a.create(
            WORKSPACE,
            "Concurrent",
            "Same request",
            "page",
            Some("concurrent"),
        )
        .unwrap()
    });
    let b = std::thread::spawn(move || {
        b.create(
            WORKSPACE,
            "Concurrent",
            "Same request",
            "page",
            Some("concurrent"),
        )
        .unwrap()
    });
    let outcomes = [a.join().unwrap(), b.join().unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| result["alreadyExists"] == false)
            .count(),
        1
    );
    assert_eq!(
        client.success("affine_list_documents", json!({"workspaceId":WORKSPACE}))["total"],
        5
    );
    let empty = json!({"workspaceId":WORKSPACE,"documentId":"empty","title":"Empty","content":""});
    client.success("affine_create_document", empty.clone());
    assert_eq!(
        client.success("affine_create_document", empty)["alreadyExists"],
        true
    );

    let mut read_only = Client::start(&storage.data_dir, false);
    assert_eq!(
        read_only.call(
            "affine_create_document",
            json!({"workspaceId":WORKSPACE,"title":"Disabled","content":"No write"})
        )["isError"],
        true
    );
    for (name, args) in [
        (
            "affine_draw_pen",
            json!({"strokes":[{"id":"pen","points":[[0,0],[100,100]]}]}),
        ),
        (
            "affine_erase_canvas",
            json!({"strokeIds":["pen"],"confirm":true}),
        ),
        (
            "affine_import_media",
            json!({"media":[{"id":"image","filePath":"sample.png","position":[0,0]}]}),
        ),
    ] {
        let mut args = args;
        args["workspaceId"] = json!(WORKSPACE);
        args["documentId"] = json!("mcp-edgeless");
        let result = read_only.call(name, args);
        assert_eq!(result["isError"], true);
        assert!(
            result["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains("Writes disabled")
        );
    }
    drop(read_only);

    canvas_journey::run(&mut client, &storage);
    drawing_journey::run(&mut client, &storage);
    document_journey::run(&mut client, &storage);
    organization_journey::run(&mut client, &storage);
    workspace_journey::run(&mut client, &storage);

    // Simulate an AFFiNE process using an owned copy, never touch the user's application.
    let fake_app = storage.data_dir.join(if cfg!(windows) {
        "AFFiNE.exe"
    } else {
        "AFFiNE"
    });
    fs::copy(env!("CARGO_BIN_EXE_affine-local-mcp"), &fake_app).unwrap();
    let mut fake = Command::new(fake_app)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let guarded = client.call(
        "affine_create_document",
        json!({"workspaceId":WORKSPACE,"title":"Blocked","content":"Must not persist"}),
    );
    let diagram_guarded = client.call("affine_create_diagram", json!({"workspaceId":WORKSPACE,"title":"Blocked diagram","nodes":[{"id":"x","text":"Must not persist"}]}));
    let edit_guarded = client.call("affine_edit_canvas", json!({"workspaceId":WORKSPACE,"documentId":"native-diagram","nodes":[{"id":"start","text":"Must not persist"}]}));
    let pen_guarded = client.call("affine_draw_pen",json!({"workspaceId":WORKSPACE,"documentId":"native-diagram","strokes":[{"id":"pen","points":[[0,0],[100,100]]}]}));
    let erase_guarded = client.call("affine_erase_canvas",json!({"workspaceId":WORKSPACE,"documentId":"native-diagram","strokeIds":["pen"],"confirm":true}));
    let media_guarded = client.call("affine_import_media",json!({"workspaceId":WORKSPACE,"documentId":"native-diagram","media":[{"id":"image","filePath":"sample.png","position":[0,0]}]}));
    let update_guarded = client.call(
        "affine_update_document",
        json!({"workspaceId":WORKSPACE,"documentId":"managed-page","title":"Blocked"}),
    );
    let delete_guarded = client.call(
        "affine_delete_document",
        json!({"workspaceId":WORKSPACE,"documentId":"managed-page","confirm":true}),
    );
    let restore_guarded = client.call(
        "affine_restore_document",
        json!({"workspaceId":WORKSPACE,"documentId":"managed-page"}),
    );
    let tag_guarded = client.call(
        "affine_manage_tag",
        json!({"workspaceId":WORKSPACE,"action":"create","name":"Blocked"}),
    );
    let tags_guarded = client.call(
        "affine_set_document_tags",
        json!({"workspaceId":WORKSPACE,"documentId":"managed-page","tagIds":[]}),
    );
    let collection_guarded = client.call(
        "affine_manage_collection",
        json!({"workspaceId":WORKSPACE,"action":"create","name":"Blocked"}),
    );
    let folder_guarded = client.call(
        "affine_manage_folder",
        json!({"workspaceId":WORKSPACE,"action":"create","kind":"folder","name":"Blocked"}),
    );
    let workspace_create_guarded =
        client.call("affine_create_workspace", json!({"name":"Blocked"}));
    let workspace_delete_guarded = client.call("affine_delete_workspace", json!({"workspaceId":"guard-workspace","confirmName":"Concurrent workspace","confirm":true}));
    let _ = fake.kill();
    let _ = fake.wait();
    assert_eq!(guarded["isError"], true);
    assert_eq!(diagram_guarded["isError"], true);
    assert_eq!(edit_guarded["isError"], true);
    for guarded in [
        &pen_guarded,
        &erase_guarded,
        &media_guarded,
        &update_guarded,
        &delete_guarded,
        &restore_guarded,
        &tag_guarded,
        &tags_guarded,
        &collection_guarded,
        &folder_guarded,
        &workspace_create_guarded,
        &workspace_delete_guarded,
    ] {
        assert_eq!(guarded["isError"], true);
        assert!(
            guarded["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains("Quit AFFiNE")
        );
    }
    assert!(
        guarded["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("Quit AFFiNE")
    );
}
