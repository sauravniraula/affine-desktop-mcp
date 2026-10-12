use crate::{
    Client,
    common::{WORKSPACE, db_path},
    document_journey::{database_state, stored_doc},
};
use affine_local_mcp::{documents, organization::ManageFolder, storage::Storage};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::collections::HashSet;
use yrs::{Array, Doc, Map, MapPrelim, MapRef, Out, ReadTxn, SharedRef, TextPrelim, Transact};

fn call(client: &mut Client, tool: &str, mut args: Value) -> Value {
    args["workspaceId"] = json!(WORKSPACE);
    client.success(tool, args)
}
fn denied(client: &mut Client, tool: &str, mut args: Value) {
    args["workspaceId"] = json!(WORKSPACE);
    assert_eq!(client.call(tool, args)["isError"], true);
}
fn find(list: &Value, key: &str, id: &str) -> Value {
    list[key]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .unwrap()
        .clone()
}

fn native_tag(root: &Doc, id: &str) -> MapRef {
    let txn = root.transact();
    let meta = txn.get_map("meta").unwrap();
    let Out::YMap(properties) = meta.get(&txn, "properties").unwrap() else {
        panic!("Native properties must be a Y.Map")
    };
    let Out::YMap(tags) = properties.get(&txn, "tags").unwrap() else {
        panic!("Native tags must be a Y.Map")
    };
    let Out::YArray(options) = tags.get(&txn, "options").unwrap() else {
        panic!("Native tag options must be a Y.Array")
    };
    options
        .iter(&txn)
        .find_map(|value| match value {
            Out::YMap(map) if map.get(&txn, "id").unwrap().to_string(&txn) == id => Some(map),
            _ => None,
        })
        .unwrap()
}

// Fixture-only seeding: keep native shared types rather than round-tripping through JSON.
fn save_fixture_doc(db: &Connection, id: &str, doc: &Doc) {
    assert_eq!(
        db.execute(
            "UPDATE snapshots SET data=?2 WHERE doc_id=?1",
            params![
                id,
                doc.transact()
                    .encode_state_as_update_v1(&Default::default())
            ],
        )
        .unwrap(),
        1
    );
}

pub fn run(client: &mut Client, storage: &Storage) {
    let db = Connection::open(db_path(storage)).unwrap();
    db.execute(
        "INSERT INTO updates VALUES (?1,?2,?3)",
        params![
            WORKSPACE,
            include_bytes!("../fixtures/organization-root-update.bin").as_slice(),
            "2026-01-01 00:00:00.002"
        ],
    )
    .unwrap();
    for (id, data) in [
        (
            "db$folders",
            include_bytes!("../fixtures/organization-folders.bin").as_slice(),
        ),
        (
            "db$pinnedCollections",
            include_bytes!("../fixtures/organization-pins.bin").as_slice(),
        ),
    ] {
        db.execute(
            "INSERT INTO snapshots(doc_id,data,updated_at) VALUES (?1,?2,?3)",
            params![id, data, "2026-01-01 00:00:00.002"],
        )
        .unwrap();
    }
    let before = database_state(&db);
    assert_eq!(
        call(client, "affine_list_tags", json!({}))["tags"][0]["value"],
        "Independent tag"
    );
    assert_eq!(
        call(client, "affine_list_collections", json!({}))["collections"][0]["name"],
        "Independent collection"
    );
    let folders = call(client, "affine_list_folders", json!({"limit":2}));
    assert_eq!(folders["total"], 4); // ORM tombstones are not visible.
    assert_eq!(folders["hasMore"], true);
    assert_eq!(folders["nextOffset"], 2);
    assert_eq!(
        call(client, "affine_list_folders", json!({"offset":2,"limit":2}))["hasMore"],
        false
    );
    assert_eq!(database_state(&db), before);

    let root = stored_doc(&db, WORKSPACE);
    let tag = native_tag(&root, "js-tag");
    let tag_id = tag.hook().id().clone();
    let original_tag = find(
        &call(client, "affine_list_tags", json!({})),
        "tags",
        "js-tag",
    );
    let (extension_id, text_id) = {
        let mut txn = root.transact_mut();
        let extension = tag.insert(&mut txn, "nativeExtension", MapPrelim::default());
        let text = extension.insert(&mut txn, "text", TextPrelim::new("preserve native text"));
        (extension.hook().id().clone(), text.hook().id().clone())
    };
    save_fixture_doc(&db, WORKSPACE, &root);
    assert_eq!(
        call(
            client,
            "affine_manage_tag",
            json!({"action":"update","tagId":"js-tag","name":"Renamed tag 日本語"}),
        )["changed"],
        true
    );
    let renamed_tag = find(
        &call(client, "affine_list_tags", json!({})),
        "tags",
        "js-tag",
    );
    assert_eq!(renamed_tag["color"], original_tag["color"]);
    assert_eq!(
        call(
            client,
            "affine_manage_tag",
            json!({"action":"update","tagId":"js-tag","color":"#123456"}),
        )["changed"],
        true
    );
    let updated_tag = find(
        &call(client, "affine_list_tags", json!({})),
        "tags",
        "js-tag",
    );
    assert_eq!(updated_tag["value"], "Renamed tag 日本語");
    assert_eq!(updated_tag["color"], "#123456");
    assert_eq!(updated_tag["createDate"], original_tag["createDate"]);
    for (key, value) in original_tag.as_object().unwrap() {
        if !["value", "color", "updateDate"].contains(&key.as_str()) {
            assert_eq!(&updated_tag[key], value);
        }
    }
    assert_eq!(
        updated_tag["nativeExtension"]["text"],
        "preserve native text"
    );
    let root = stored_doc(&db, WORKSPACE);
    let tag = native_tag(&root, "js-tag");
    let txn = root.transact();
    assert_eq!(tag.hook().id(), &tag_id);
    let Out::YMap(extension) = tag.get(&txn, "nativeExtension").unwrap() else {
        panic!("Tag extension must remain a native Y.Map")
    };
    assert_eq!(extension.hook().id(), &extension_id);
    let Out::YText(text) = extension.get(&txn, "text").unwrap() else {
        panic!("Nested extension text must remain a native Y.Text")
    };
    assert_eq!(text.hook().id(), &text_id);
    drop(txn);
    let create_tag = json!({"action":"create","tagId":"new-tag","name":"Project"});
    assert_eq!(
        call(client, "affine_manage_tag", create_tag.clone())["changed"],
        true
    );
    let before = database_state(&db);
    assert_eq!(
        call(client, "affine_manage_tag", create_tag)["changed"],
        false
    );
    assert_eq!(database_state(&db), before);
    denied(
        client,
        "affine_manage_tag",
        json!({"action":"create","tagId":"new-tag","name":"Conflict"}),
    );
    denied(
        client,
        "affine_manage_tag",
        json!({"action":"update","tagId":"js-tag","color":"javascript:bad"}),
    );
    call(
        client,
        "affine_set_document_tags",
        json!({"documentId":"fixture-page","tagIds":["js-tag","new-tag"]}),
    );
    let list = call(client, "affine_list_documents", json!({}));
    assert_eq!(
        find(&list, "documents", "fixture-page")["tagIds"],
        json!(["js-tag", "new-tag"])
    );
    let before = database_state(&db);
    assert_eq!(
        call(
            client,
            "affine_set_document_tags",
            json!({"documentId":"fixture-page","tagIds":["js-tag","new-tag"]})
        )["changed"],
        false
    );
    denied(
        client,
        "affine_set_document_tags",
        json!({"documentId":"fixture-page","tagIds":["missing"]}),
    );
    denied(
        client,
        "affine_set_document_tags",
        json!({"documentId":"fixture-page","tagIds":["new-tag","new-tag"]}),
    );
    denied(
        client,
        "affine_set_document_tags",
        json!({"documentId":"fixture-trash","tagIds":[]}),
    );
    assert_eq!(database_state(&db), before);

    let original_collection =
        call(client, "affine_list_collections", json!({}))["collections"][0].clone();
    call(
        client,
        "affine_manage_collection",
        json!({"action":"update","collectionId":"js-collection","name":"Renamed collection","addDocumentIds":["managed-page"]}),
    );
    let collection = find(
        &call(client, "affine_list_collections", json!({})),
        "collections",
        "js-collection",
    );
    assert_eq!(collection["rules"], original_collection["rules"]);
    assert_eq!(collection["extra"], original_collection["extra"]);
    assert_eq!(
        collection["allowList"],
        json!(["fixture-page", "managed-page"])
    );
    call(
        client,
        "affine_manage_collection",
        json!({"action":"update","collectionId":"js-collection","removeDocumentIds":["fixture-page"]}),
    );
    let create_collection = json!({"action":"create","collectionId":"new-collection","name":"Project collection","addDocumentIds":["fixture-page","managed-page"]});
    call(
        client,
        "affine_manage_collection",
        create_collection.clone(),
    );
    let before = database_state(&db);
    assert_eq!(
        call(
            client,
            "affine_manage_collection",
            create_collection.clone()
        )["changed"],
        false
    );
    assert_eq!(database_state(&db), before);
    denied(
        client,
        "affine_manage_collection",
        json!({"action":"update","collectionId":"new-collection","addDocumentIds":["orphan"]}),
    );
    denied(
        client,
        "affine_manage_collection",
        json!({"action":"update","collectionId":"new-collection","addDocumentIds":["fixture-trash"]}),
    );
    denied(
        client,
        "affine_manage_collection",
        json!({"action":"update","collectionId":"new-collection","addDocumentIds":["fixture-page"],"removeDocumentIds":["fixture-page"]}),
    );

    let folder =
        json!({"action":"create","nodeId":"project-folder","kind":"folder","name":"Project"});
    call(client, "affine_manage_folder", folder.clone());
    call(
        client,
        "affine_manage_folder",
        json!({"action":"create","nodeId":"sub-folder","kind":"folder","name":"Subfolder","parentId":"project-folder"}),
    );
    let doc_link = json!({"action":"create","nodeId":"doc-link","kind":"doc","targetId":"managed-page","parentId":"sub-folder"});
    call(client, "affine_manage_folder", doc_link.clone());
    call(
        client,
        "affine_manage_folder",
        json!({"action":"create","nodeId":"tag-link","kind":"tag","targetId":"new-tag","parentId":"sub-folder"}),
    );
    call(
        client,
        "affine_manage_folder",
        json!({"action":"create","nodeId":"collection-link","kind":"collection","targetId":"new-collection","parentId":"sub-folder"}),
    );
    let before = database_state(&db);
    assert_eq!(
        call(client, "affine_manage_folder", folder)["changed"],
        false
    );
    assert_eq!(database_state(&db), before);
    call(
        client,
        "affine_manage_folder",
        json!({"action":"update","nodeId":"js-doc-link","parentId":"sub-folder","index":"a0V"}),
    );
    call(
        client,
        "affine_manage_folder",
        json!({"action":"update","nodeId":"project-folder","name":"Renamed project"}),
    );
    let folder_list = call(client, "affine_list_folders", json!({}));
    assert_eq!(
        find(&folder_list, "folders", "project-folder")["data"],
        "Renamed project"
    );
    assert_eq!(
        find(&folder_list, "folders", "js-doc-link")["parentId"],
        "sub-folder"
    );
    assert_eq!(find(&folder_list, "folders", "js-doc-link")["index"], "a0V");
    let before = database_state(&db);
    for args in [
        json!({"action":"update","nodeId":"project-folder","parentId":"sub-folder"}),
        json!({"action":"update","nodeId":"project-folder","parentId":"project-folder"}),
        json!({"action":"update","nodeId":"doc-link","parentId":""}),
        json!({"action":"update","nodeId":"sub-folder","parentId":"doc-link"}),
        json!({"action":"update","nodeId":"doc-link","kind":"folder","name":"Cannot convert"}),
        json!({"action":"create","kind":"doc","targetId":"fixture-page"}),
        json!({"action":"create","kind":"doc","targetId":"missing","parentId":"js-folder"}),
        json!({"action":"create","kind":"folder","name":"Bad index","index":"invalid"}),
    ] {
        denied(client, "affine_manage_folder", args);
    }
    assert_eq!(database_state(&db), before);
    call(
        client,
        "affine_manage_folder",
        json!({"action":"update","nodeId":"sub-folder","parentId":""}),
    );
    assert_eq!(
        find(
            &call(client, "affine_list_folders", json!({})),
            "folders",
            "sub-folder"
        )["parentId"],
        Value::Null
    );
    call(
        client,
        "affine_manage_folder",
        json!({"action":"update","nodeId":"sub-folder","parentId":"project-folder"}),
    );

    // Tag cleanup applies to trashed assignments too, and preserves unrelated tags/properties.
    call(
        client,
        "affine_set_document_tags",
        json!({"documentId":"managed-page","tagIds":["js-tag"]}),
    );
    call(
        client,
        "affine_delete_document",
        json!({"documentId":"managed-page","confirm":true}),
    );
    // Stable-ID retries remain no-ops when unchanged references have since been trashed.
    let before = database_state(&db);
    assert_eq!(
        call(
            client,
            "affine_manage_collection",
            create_collection.clone()
        )["changed"],
        false
    );
    assert_eq!(
        call(client, "affine_manage_folder", doc_link.clone())["changed"],
        false
    );
    assert_eq!(database_state(&db), before);
    for args in [
        json!({"action":"create","collectionId":"new-collection","name":"Conflict","addDocumentIds":["fixture-page","managed-page"]}),
        json!({"action":"create","collectionId":"new-collection","name":"Project collection","addDocumentIds":["fixture-trash"]}),
        json!({"action":"create","collectionId":"inactive-collection","name":"Inactive","addDocumentIds":["managed-page"]}),
        json!({"action":"update","collectionId":"new-collection","addDocumentIds":["managed-page"]}),
        json!({"action":"update","collectionId":"new-collection","addDocumentIds":["fixture-trash"]}),
    ] {
        denied(client, "affine_manage_collection", args);
    }
    for args in [
        json!({"action":"create","nodeId":"doc-link","kind":"doc","targetId":"fixture-page","parentId":"sub-folder"}),
        json!({"action":"create","nodeId":"doc-link","kind":"doc","targetId":"managed-page","parentId":"js-folder"}),
        json!({"action":"create","nodeId":"inactive-link","kind":"doc","targetId":"managed-page","parentId":"sub-folder"}),
        json!({"action":"update","nodeId":"doc-link","targetId":"fixture-trash"}),
    ] {
        denied(client, "affine_manage_folder", args);
    }
    assert_eq!(database_state(&db), before);
    denied(
        client,
        "affine_manage_tag",
        json!({"action":"delete","tagId":"js-tag"}),
    );
    let before = database_state(&db);
    db.execute_batch("CREATE TRIGGER fail_organization BEFORE INSERT ON updates WHEN NEW.doc_id='test-workspace' BEGIN SELECT RAISE(ABORT,'organization rollback'); END;").unwrap();
    denied(
        client,
        "affine_manage_tag",
        json!({"action":"delete","tagId":"js-tag","confirm":true}),
    );
    assert_eq!(database_state(&db), before);
    denied(
        client,
        "affine_manage_collection",
        json!({"action":"delete","collectionId":"js-collection","confirm":true}),
    );
    assert_eq!(database_state(&db), before);
    db.execute_batch("DROP TRIGGER fail_organization").unwrap();
    call(
        client,
        "affine_manage_tag",
        json!({"action":"delete","tagId":"js-tag","confirm":true}),
    );
    let list = call(client, "affine_list_documents", json!({}));
    assert_eq!(
        find(&list, "documents", "fixture-page")["tagIds"],
        json!(["new-tag"])
    );
    assert_eq!(
        find(
            &call(client, "affine_list_trash", json!({})),
            "documents",
            "managed-page"
        )["tagIds"],
        json!([])
    );
    assert!(
        !call(client, "affine_list_folders", json!({}))["folders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "js-tag-link")
    );
    let before = database_state(&db);
    assert_eq!(
        call(
            client,
            "affine_manage_tag",
            json!({"action":"delete","tagId":"js-tag","confirm":true})
        )["changed"],
        false
    );
    assert_eq!(database_state(&db), before);
    call(
        client,
        "affine_restore_document",
        json!({"documentId":"managed-page"}),
    );
    call(
        client,
        "affine_manage_collection",
        json!({"action":"delete","collectionId":"js-collection","confirm":true}),
    );
    assert_eq!(
        call(client, "affine_list_collections", json!({}))["total"],
        1
    );
    assert!(documents::root_json(&stored_doc(&db,"db$pinnedCollections"),"js-collection").unwrap()["$$DELETED"].as_bool().unwrap());
    assert!(
        !call(client, "affine_list_folders", json!({}))["folders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "js-collection-link")
    );
    let before = database_state(&db);
    assert_eq!(
        call(
            client,
            "affine_manage_collection",
            json!({"action":"delete","collectionId":"js-collection","confirm":true})
        )["changed"],
        false
    );
    assert_eq!(database_state(&db), before);

    // Absent registry entities may still have stale folder links and/or pinned rows.
    let root = stored_doc(&db, WORKSPACE);
    {
        let mut txn = root.transact_mut();
        let meta = txn.get_map("meta").unwrap();
        let Out::YArray(pages) = meta.get(&txn, "pages").unwrap() else {
            panic!()
        };
        let page = pages
            .iter(&txn)
            .find_map(|value| match value {
                Out::YMap(page)
                    if page.get(&txn, "id").unwrap().to_string(&txn) == "fixture-page" =>
                {
                    Some(page)
                }
                _ => None,
            })
            .unwrap();
        let Out::YArray(tags) = page.get(&txn, "tags").unwrap() else {
            panic!()
        };
        tags.push_back(&mut txn, "missing-assigned-tag");
    }
    save_fixture_doc(&db, WORKSPACE, &root);
    let cleanup = json!({"action":"delete","tagId":"missing-assigned-tag","confirm":true});
    assert_eq!(
        call(client, "affine_manage_tag", cleanup.clone())["changed"],
        true
    );
    assert_eq!(
        find(
            &call(client, "affine_list_documents", json!({})),
            "documents",
            "fixture-page"
        )["tagIds"],
        json!(["new-tag"])
    );
    let before = database_state(&db);
    assert_eq!(call(client, "affine_manage_tag", cleanup)["changed"], false);
    assert_eq!(database_state(&db), before);
    let root_before: Vec<_> = database_state(&db)
        .into_iter()
        .map(|rows| {
            rows.into_iter()
                .filter(|row| row.0 == WORKSPACE)
                .collect::<Vec<_>>()
        })
        .collect();
    let stale = [
        ("tag", "missing-tag", true, false),
        ("collection", "missing-linked-collection", true, false),
        ("collection", "missing-pinned-collection", false, true),
        ("collection", "missing-linked-pinned-collection", true, true),
    ];
    let folders = stored_doc(&db, "db$folders");
    let pinned = stored_doc(&db, "db$pinnedCollections");
    for (kind, id, link, pin) in stale {
        if link {
            let row = folders.get_or_insert_map(format!("stale-{id}"));
            let mut txn = folders.transact_mut();
            for (key, value) in [
                ("id", format!("stale-{id}")),
                ("type", kind.to_owned()),
                ("data", id.to_owned()),
                ("parentId", "js-folder".to_owned()),
                ("index", "a0".to_owned()),
            ] {
                row.insert(&mut txn, key, value);
            }
        }
        if pin {
            let row = pinned.get_or_insert_map(id);
            let mut txn = pinned.transact_mut();
            row.insert(&mut txn, "id", id);
            row.insert(&mut txn, "index", "a0");
        }
    }
    save_fixture_doc(&db, "db$folders", &folders);
    save_fixture_doc(&db, "db$pinnedCollections", &pinned);
    for (kind, id, link, pin) in stale {
        let tool = if kind == "tag" {
            "affine_manage_tag"
        } else {
            "affine_manage_collection"
        };
        let field = if kind == "tag" {
            "tagId"
        } else {
            "collectionId"
        };
        let mut args = json!({"action":"delete","confirm":true});
        args[field] = json!(id);
        assert_eq!(call(client, tool, args.clone())["changed"], true);
        if link {
            assert_eq!(
                documents::root_json(&stored_doc(&db, "db$folders"), &format!("stale-{id}"))
                    .unwrap()["$$DELETED"],
                true
            );
        }
        if pin {
            assert_eq!(
                documents::root_json(&stored_doc(&db, "db$pinnedCollections"), id).unwrap()["$$DELETED"],
                true
            );
        }
        let before = database_state(&db);
        assert_eq!(call(client, tool, args)["changed"], false);
        assert_eq!(database_state(&db), before);
    }
    let root_after: Vec<_> = database_state(&db)
        .into_iter()
        .map(|rows| {
            rows.into_iter()
                .filter(|row| row.0 == WORKSPACE)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(root_after, root_before);
    let before = database_state(&db);

    // Removing a folder recursively removes tree nodes, not their target documents/entities.
    denied(
        client,
        "affine_manage_folder",
        json!({"action":"delete","nodeId":"project-folder"}),
    );
    db.execute_batch("CREATE TRIGGER fail_folder BEFORE INSERT ON updates WHEN NEW.doc_id='db$folders' BEGIN SELECT RAISE(ABORT,'folder rollback'); END;").unwrap();
    denied(
        client,
        "affine_manage_folder",
        json!({"action":"delete","nodeId":"project-folder","confirm":true}),
    );
    assert_eq!(database_state(&db), before);
    db.execute_batch("DROP TRIGGER fail_folder").unwrap();
    call(
        client,
        "affine_manage_folder",
        json!({"action":"delete","nodeId":"project-folder","confirm":true}),
    );
    let folders = call(client, "affine_list_folders", json!({}));
    assert_eq!(folders["total"], 1);
    assert_eq!(folders["folders"][0]["id"], "js-folder");
    call(
        client,
        "affine_read_document",
        json!({"documentId":"fixture-page"}),
    );
    call(
        client,
        "affine_read_document",
        json!({"documentId":"managed-page"}),
    );
    assert_eq!(call(client, "affine_list_tags", json!({}))["total"], 1);
    assert_eq!(
        call(client, "affine_list_collections", json!({}))["total"],
        1
    );
    assert_eq!(
        documents::root_json(&stored_doc(&db, WORKSPACE), "meta").unwrap()["properties"]["unrelated"],
        "preserve workspace properties"
    );
    assert_eq!(
        documents::root_json(&stored_doc(&db, WORKSPACE), "setting").unwrap()["unrelated"],
        "preserve workspace settings"
    );
    let before = database_state(&db);
    assert_eq!(
        call(
            client,
            "affine_manage_folder",
            json!({"action":"delete","nodeId":"project-folder","confirm":true})
        )["changed"],
        false
    );
    assert_eq!(database_state(&db), before);

    let mut read_only = Client::start(&storage.data_dir, false);
    for (tool, args) in [
        (
            "affine_manage_tag",
            json!({"action":"create","name":"Blocked"}),
        ),
        (
            "affine_manage_collection",
            json!({"action":"create","name":"Blocked"}),
        ),
        (
            "affine_manage_folder",
            json!({"action":"create","kind":"folder","name":"Blocked"}),
        ),
        (
            "affine_set_document_tags",
            json!({"documentId":"fixture-page","tagIds":[]}),
        ),
    ] {
        denied(&mut read_only, tool, args);
    }
    for tool in [
        "affine_list_tags",
        "affine_list_collections",
        "affine_list_folders",
    ] {
        call(&mut read_only, tool, json!({}));
        denied(client, tool, json!({"limit":0}));
    }
    let mut restricted = storage.clone();
    restricted.allowlist = Some(HashSet::new());
    assert!(
        restricted
            .list_organization(WORKSPACE, "folders", 0, 100)
            .is_err()
    );
    let params: ManageFolder = serde_json::from_value(
        json!({"workspaceId":WORKSPACE,"action":"create","kind":"folder","name":"Blocked"}),
    )
    .unwrap();
    assert!(restricted.manage_folder(&params).is_err());
    assert_eq!(database_state(&db), before);
}
