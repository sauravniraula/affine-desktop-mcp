use affine_local_mcp::{
    canvas::{self, CreateDiagram, Direction, Edge, Node},
    documents,
    storage::build_document,
};
use serde_json::{Value, json};
use yrs::{Doc, Map, Out, Transact, Update, types::ToJson, updates::decoder::Decode};

fn node(value: Value) -> Node {
    serde_json::from_value(value).unwrap()
}
fn edge(value: Value) -> Edge {
    serde_json::from_value(value).unwrap()
}

#[test]
fn every_native_primitive_persists_its_model_id_and_repairs_missing_ids_on_upsert() {
    let doc = build_document("identity", "Identity", "", "edgeless").unwrap();
    let elements = canvas::elements(&doc).unwrap();
    for kind in ["brush", "shape", "connector"] {
        canvas::upsert(&doc, &elements, kind, json!({"type":kind})).unwrap();
        let map = {
            let txn = doc.transact();
            let Some(Out::YMap(map)) = elements.get(&txn, kind) else {
                panic!()
            };
            assert_eq!(
                map.get(&txn, "id").map(|v| v.to_string(&txn)),
                Some(kind.to_owned()),
                "AFFiNE initializes model.id from the nested property, not the container key"
            );
            map
        };
        map.remove(&mut doc.transact_mut(), "id");
        canvas::upsert(&doc, &elements, kind, json!({"type":kind})).unwrap();
        let txn = doc.transact();
        assert_eq!(
            map.get(&txn, "id").map(|v| v.to_string(&txn)),
            Some(kind.to_owned())
        );
    }
}

#[test]
fn upsert_rejects_conflicting_ids_without_mutating_the_element() {
    let doc = build_document("identity", "Identity", "", "edgeless").unwrap();
    let elements = canvas::elements(&doc).unwrap();
    canvas::upsert(&doc, &elements, "pen", json!({"type":"brush"})).unwrap();
    let before = elements.to_json(&doc.transact());
    assert!(canvas::upsert(&doc, &elements, "pen", json!({"type":"brush","id":"other"})).is_err());
    assert_eq!(elements.to_json(&doc.transact()), before);
    let map = {
        let txn = doc.transact();
        let Some(Out::YMap(map)) = elements.get(&txn, "pen") else {
            panic!()
        };
        map
    };
    map.insert(&mut doc.transact_mut(), "id", "other");
    let before = elements.to_json(&doc.transact());
    assert!(canvas::upsert(&doc, &elements, "pen", json!({"type":"brush"})).is_err());
    assert_eq!(elements.to_json(&doc.transact()), before);
}

#[test]
fn independent_canvas_fixtures_include_the_ids_required_by_blocksuite_models() {
    for bytes in [
        include_bytes!("fixtures/native-canvas.bin").as_slice(),
        include_bytes!("fixtures/native-drawing.bin").as_slice(),
    ] {
        let doc = Doc::new();
        doc.transact_mut()
            .apply_update(Update::decode_v1(bytes).unwrap())
            .unwrap();
        let map = canvas::elements(&doc).unwrap();
        let txn = doc.transact();
        for (id, value) in map.iter(&txn) {
            let Out::YMap(element) = value else { panic!() };
            assert_eq!(
                element.get(&txn, "id").map(|v| v.to_string(&txn)),
                Some(id.to_owned()),
                "Invalid native fixture element: {id}"
            );
        }
    }
}

#[test]
fn reads_and_edits_independently_generated_javascript_native_shapes_and_connectors() {
    let doc = Doc::new();
    doc.transact_mut()
        .apply_update(Update::decode_v1(include_bytes!("fixtures/native-canvas.bin")).unwrap())
        .unwrap();
    let elements = canvas::elements(&doc).unwrap();
    let raw = serde_json::to_value(elements.to_json(&doc.transact())).unwrap();
    assert_eq!(raw.as_object().unwrap().len(), 3);
    assert_eq!(raw["js-start"]["type"], "shape");
    assert_eq!(raw["js-start"]["shapeType"], "ellipse");
    assert_eq!(raw["js-start"]["text"], "Independent shape 🌏");
    assert_eq!(raw["js-arrow"]["source"]["id"], "js-start");
    assert_eq!(raw["js-arrow"]["target"]["id"], "js-decision");
    let updated =
        node(json!({"id":"js-start","text":"Rust edited JS Y.Text 🦀","shape":"ellipse"}));
    canvas::upsert(
        &doc,
        &elements,
        &updated.id,
        canvas::shape_props(&updated, [100.0, 100.0, 260.0, 120.0]),
    )
    .unwrap();
    let rendered = documents::render(&doc).unwrap();
    assert!(rendered.markdown.contains("Rust edited JS Y.Text 🦀"));
    assert!(
        rendered
            .markdown
            .contains("[Canvas connector js-arrow] Next step")
    );
    assert_eq!(rendered.block_count, 2);
}

#[test]
fn native_shapes_and_connector_labels_are_editable_y_text_not_paragraphs() {
    let doc = build_document("native", "Diagram", "", "edgeless").unwrap();
    let elements = canvas::elements(&doc).unwrap();
    let a = node(json!({"id":"a","text":"Editable 🌏","shape":"diamond"}));
    let b = node(json!({"id":"b","text":"Next","shape":"ellipse"}));
    canvas::upsert(
        &doc,
        &elements,
        "a",
        canvas::shape_props(&a, [0.0, 0.0, 260.0, 120.0]),
    )
    .unwrap();
    canvas::upsert(
        &doc,
        &elements,
        "b",
        canvas::shape_props(&b, [400.0, 0.0, 260.0, 120.0]),
    )
    .unwrap();
    let arrow = edge(json!({"id":"arrow","source":"a","target":"b","text":"Go"}));
    canvas::upsert(
        &doc,
        &elements,
        "arrow",
        canvas::edge_props(&arrow, [0.0, 0.0, 260.0, 120.0], [400.0, 0.0, 260.0, 120.0]),
    )
    .unwrap();
    let original_text = {
        let txn = doc.transact();
        let Some(Out::YMap(shape)) = elements.get(&txn, "a") else {
            panic!("Shape must be a Y.Map")
        };
        let Some(Out::YText(text)) = shape.get(&txn, "text") else {
            panic!("Label must be a Y.Text")
        };
        let Some(Out::YMap(connector)) = elements.get(&txn, "arrow") else {
            panic!("Connector must be a Y.Map")
        };
        assert!(matches!(connector.get(&txn, "text"), Some(Out::YText(_))));
        text
    };
    let updated = node(json!({"id":"a","text":"Updated 🦀","shape":"diamond"}));
    canvas::upsert(
        &doc,
        &elements,
        "a",
        canvas::shape_props(&updated, [-100.0, 20.0, 260.0, 120.0]),
    )
    .unwrap();
    let txn = doc.transact();
    let Some(Out::YMap(shape)) = elements.get(&txn, "a") else {
        panic!()
    };
    let Some(Out::YText(text)) = shape.get(&txn, "text") else {
        panic!()
    };
    assert_eq!(
        text, original_text,
        "Updating labels must preserve their CRDT identity"
    );
    let raw = serde_json::to_value(elements.to_json(&txn)).unwrap();
    assert_eq!(raw["a"]["text"], "Updated 🦀");
    assert_eq!(raw["arrow"]["source"], json!({"id":"a","position":[1,0.5]}));
    assert_eq!(raw["arrow"]["target"], json!({"id":"b","position":[0,0.5]}));
    assert_eq!(raw["arrow"]["mode"], 1);
    assert_eq!(raw["arrow"]["rearEndpointStyle"], "Arrow");
    drop(txn);
    let rendered = documents::render(&doc).unwrap();
    assert_eq!(rendered.block_count, 2);
    assert!(rendered.markdown.contains("[Canvas shape a] Updated 🦀"));
    assert!(rendered.markdown.contains("[Canvas connector arrow] Go"));
    shape.insert(&mut doc.transact_mut(), "lockedBySelf", true);
    let locked_update = node(json!({"id":"a","text":"Must not replace a locked label"}));
    assert!(
        canvas::upsert(
            &doc,
            &elements,
            "a",
            canvas::shape_props(&locked_update, [0.0, 0.0, 260.0, 120.0])
        )
        .is_err()
    );
    assert!(
        documents::render(&doc)
            .unwrap()
            .markdown
            .contains("Updated 🦀")
    );
}

#[test]
fn layered_layout_separates_branches_and_supports_explicit_cycles() {
    let nodes: Vec<_> = ["start", "decision", "yes", "no", "end"]
        .iter()
        .map(|id| node(json!({"id":id,"text":id})))
        .collect();
    let edges: Vec<_> = [
        ("start", "decision"),
        ("decision", "yes"),
        ("decision", "no"),
        ("yes", "end"),
        ("no", "end"),
    ]
    .iter()
    .enumerate()
    .map(|(i, (source, target))| {
        edge(json!({"id":format!("edge-{i}"),"source":source,"target":target}))
    })
    .collect();
    let tb = canvas::layout(&nodes, &edges, Direction::Tb).unwrap();
    assert!(tb[0][1] < tb[1][1] && tb[1][1] < tb[2][1] && tb[2][1] < tb[4][1]);
    assert_eq!(tb[2][1], tb[3][1]);
    assert!(tb[2][0] + tb[2][2] < tb[3][0]);
    let lr = canvas::layout(&nodes, &edges, Direction::Lr).unwrap();
    assert!(lr[0][0] < lr[1][0] && lr[1][0] < lr[2][0] && lr[2][0] < lr[4][0]);
    assert!(lr[2][1] + lr[2][3] < lr[3][1]);
    let mut cyclic = edges;
    cyclic.push(edge(json!({"id":"cycle","source":"end","target":"start"})));
    assert!(canvas::layout(&nodes, &cyclic, Direction::Tb).is_err());
    let positioned: Vec<_> = nodes
        .into_iter()
        .enumerate()
        .map(|(i, mut n)| {
            n.position = Some([i as f64 * 400.0, 100.0]);
            n
        })
        .collect();
    let explicit = canvas::layout(&positioned, &cyclic, Direction::Tb).unwrap();
    assert_eq!(&explicit[4][..2], &[1600.0, 100.0]);
    assert!(canvas::layout(&[], &[], Direction::Tb).is_err());
}

#[test]
fn fractional_layer_indices_match_native_integer_key_boundaries() {
    for (last, expected) in [
        ("a0", "a1"),
        ("a0V", "a1"),
        ("az", "b00"),
        ("bzz", "c000"),
        ("Zz", "a0"),
        ("Yzz", "Z0"),
    ] {
        assert_eq!(canvas::next_index(Some(last)).unwrap(), expected);
    }
    let mut last = canvas::next_index(None).unwrap();
    let mut unique = std::collections::HashSet::new();
    for _ in 0..1000 {
        assert!(unique.insert(last.clone()));
        let next = canvas::next_index(Some(&last)).unwrap();
        assert!(next > last);
        last = next;
    }
    assert!(canvas::next_index(Some("invalid")).is_err());
    assert!(canvas::next_index(Some("")).is_err());
}

#[test]
fn invalid_geometry_labels_colors_ids_and_unknown_fields_are_rejected() {
    for value in [
        json!({"id":"../escape","text":"X"}),
        json!({"id":"x","text":"X","size":[0,120]}),
        json!({"id":"x","text":"X","position":[2000000,0]}),
        json!({"id":"x","text":"X","fontSize":100}),
        json!({"id":"x","text":"X","fillColor":"red;bad"}),
        json!({"id":"x","text":"X","fillColor":{"light":"#zzzzzz","dark":"#000000"}}),
        json!({"id":"x","text":"X".repeat(2001)}),
    ] {
        assert!(canvas::validate(&[node(value)], &[]).is_err());
    }
    let n = node(json!({"id":"x","text":"X"}));
    assert!(canvas::validate(&[n.clone(), n], &[]).is_err());
    let n = node(json!({"id":"x","text":"X"}));
    let e = edge(json!({"id":"x","source":"x","target":"other"}));
    assert!(canvas::validate(&[n], &[e]).is_err());
    assert!(
        serde_json::from_value::<Node>(json!({"id":"x","text":"X","shape":"hexagon"})).is_err()
    );
    assert!(serde_json::from_value::<Node>(json!({"id":"x","text":"X","positon":[0,0]})).is_err());
    assert!(
        serde_json::from_value::<CreateDiagram>(
            json!({"workspaceId":"x","title":"X","nodes":[],"unexpected":true})
        )
        .is_err()
    );
}
