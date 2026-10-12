//! AFFiNE 0.27.4 workspace organization: native tag/collection registries and ORM folder rows.
use crate::{
    canvas, documents,
    storage::{
        Storage, active_pages, append_update, document_entry, identifier, load_doc, optional_doc,
    },
};
use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;
use yrs::{
    Array, ArrayPrelim, ArrayRef, Doc, In, Map, MapPrelim, MapRef, Out, ReadTxn, StateVector,
    Transact, types::ToJson,
};

#[derive(Debug, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManageTag {
    pub workspace_id: String,
    pub action: Action,
    /// Optional stable ID on create; required on update/delete.
    pub tag_id: Option<String>,
    pub name: Option<String>,
    /// AFFiNE theme color token or hex color; defaults to --affine-tag-blue on create.
    pub color: Option<String>,
    /// Tag deletion requires true; removes assignments and folder links, not documents.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetDocumentTags {
    pub workspace_id: String,
    pub document_id: String,
    /// Complete replacement tag-ID list (maximum 100); [] clears assignments.
    pub tag_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManageCollection {
    pub workspace_id: String,
    pub action: Action,
    pub collection_id: Option<String>,
    pub name: Option<String>,
    /// Add active documents to the explicit allowList (maximum 200 per call).
    #[serde(default)]
    pub add_document_ids: Vec<String>,
    /// Remove explicit members. Existing smart rules are preserved, not evaluated.
    #[serde(default)]
    pub remove_document_ids: Vec<String>,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Folder,
    Doc,
    Tag,
    Collection,
}
impl NodeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Doc => "doc",
            Self::Tag => "tag",
            Self::Collection => "collection",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManageFolder {
    pub workspace_id: String,
    pub action: Action,
    /// Folder-tree node ID, not a document/tag/collection ID. Optional on create.
    pub node_id: Option<String>,
    /// Required on create; immutable afterward. Folders or shortcuts to other entities.
    pub kind: Option<NodeKind>,
    /// Folder name (create or rename). Use targetId instead for shortcuts.
    pub name: Option<String>,
    pub target_id: Option<String>,
    /// Omit on update to preserve parent; use "" to move a folder to the root.
    pub parent_id: Option<String>,
    /// Native fractional sorting key. Omit to append on create/move or retain on rename.
    pub index: Option<String>,
    /// Deleting folders recursively removes their tree nodes/shortcuts, never documents.
    #[serde(default)]
    pub confirm: bool,
}

fn name(value: &str) -> Result<&str> {
    ensure!(
        !value.trim().is_empty() && value.chars().count() <= 500,
        "name must contain 1..500 characters"
    );
    Ok(value.trim())
}

fn entity_id(action: Action, requested: Option<&str>, confirm: bool) -> Result<String> {
    ensure!(
        action != Action::Delete || confirm,
        "Deletion requires confirm=true"
    );
    let id = match requested {
        Some(id) => id.to_owned(),
        None if action == Action::Create => Uuid::new_v4().to_string(),
        None => bail!("An existing entity ID is required"),
    };
    ensure!(identifier(&id), "Invalid entity ID");
    Ok(id)
}

fn ids(values: &[String], max: usize) -> Result<()> {
    let unique: HashSet<_> = values.iter().collect();
    ensure!(
        values.len() <= max
            && unique.len() == values.len()
            && values.iter().all(|id| identifier(id)),
        "IDs must be valid, unique, and at most {max} per call"
    );
    Ok(())
}

fn nested_map(doc: &Doc, parent: &MapRef, key: &str, create: bool) -> Result<Option<MapRef>> {
    let mut txn = doc.transact_mut();
    match parent.get(&txn, key) {
        Some(Out::YMap(map)) => Ok(Some(map)),
        None if create => Ok(Some(parent.insert(&mut txn, key, MapPrelim::default()))),
        None => Ok(None),
        _ => bail!("Unsupported organization metadata: {key} must be Y.Map"),
    }
}

fn nested_array(doc: &Doc, parent: &MapRef, key: &str, create: bool) -> Result<Option<ArrayRef>> {
    let mut txn = doc.transact_mut();
    match parent.get(&txn, key) {
        Some(Out::YArray(array)) => Ok(Some(array)),
        None if create => Ok(Some(parent.insert(&mut txn, key, ArrayPrelim::default()))),
        None => Ok(None),
        _ => bail!("Unsupported organization metadata: {key} must be Y.Array"),
    }
}

fn tag_array(root: &Doc, create: bool) -> Result<Option<ArrayRef>> {
    let meta = root
        .transact()
        .get_map("meta")
        .context("Missing metadata")?;
    let Some(properties) = nested_map(root, &meta, "properties", create)? else {
        return Ok(None);
    };
    let Some(tags) = nested_map(root, &properties, "tags", create)? else {
        return Ok(None);
    };
    nested_array(root, &tags, "options", create)
}

fn collection_array(root: &Doc, create: bool) -> Result<Option<ArrayRef>> {
    let setting = if create {
        root.get_or_insert_map("setting")
    } else {
        let Some(map) = root.transact().get_map("setting") else {
            return Ok(None);
        };
        map
    };
    nested_array(root, &setting, "collections", create)
}

fn array_values(doc: &Doc, array: Option<&ArrayRef>) -> Result<Vec<Value>> {
    let Some(array) = array else {
        return Ok(Vec::new());
    };
    let value = serde_json::to_value(array.to_json(&doc.transact()))?;
    let records = value
        .as_array()
        .context("Invalid organization array")?
        .clone();
    let mut seen = HashSet::new();
    for record in &records {
        ensure!(
            record.is_object() && record["id"].as_str().is_some_and(|id| seen.insert(id)),
            "Invalid/duplicate organization record ID"
        );
    }
    Ok(records)
}

fn tags(root: &Doc) -> Result<Vec<Value>> {
    array_values(root, tag_array(root, false)?.as_ref())
}
fn collections(root: &Doc) -> Result<Vec<Value>> {
    array_values(root, collection_array(root, false)?.as_ref())
}

fn map_record(value: &Value) -> Result<MapPrelim> {
    let mut map = MapPrelim::default();
    for (key, value) in value.as_object().context("Expected a record")? {
        map.insert(
            key.as_str().into(),
            In::from(serde_json::from_value::<yrs::Any>(value.clone())?),
        );
    }
    Ok(map)
}

fn replace_record(
    doc: &Doc,
    array: &ArrayRef,
    index: Option<usize>,
    value: Option<&Value>,
    native_map: bool,
) -> Result<()> {
    let mut txn = doc.transact_mut();
    let at = index.map(|i| i as u32).unwrap_or_else(|| array.len(&txn));
    if index.is_some() {
        array.remove_range(&mut txn, at, 1);
    }
    if let Some(value) = value {
        if native_map {
            array.insert(&mut txn, at, map_record(value)?);
        } else {
            array.insert(
                &mut txn,
                at,
                serde_json::from_value::<yrs::Any>(value.clone())?,
            );
        }
    }
    Ok(())
}

fn table_json(doc: &Doc) -> Result<Value> {
    let txn = doc.transact();
    let mut rows = serde_json::Map::new();
    for (key, _) in txn.root_refs() {
        let map = txn.get_map(key).context("Unsupported ORM row")?;
        rows.insert(key.to_owned(), serde_json::to_value(map.to_json(&txn))?);
    }
    Ok(Value::Object(rows))
}

fn folder_rows(doc: &Doc) -> Result<Vec<Value>> {
    let value = table_json(doc)?;
    let mut rows = Vec::new();
    for (key, row) in value.as_object().unwrap() {
        if row["$$DELETED"] == true || row.as_object().is_some_and(|map| map.is_empty()) {
            continue;
        }
        ensure!(
            row["id"] == key.as_str(),
            "Folder row ID does not match its native key"
        );
        ensure!(
            row["type"]
                .as_str()
                .is_some_and(|kind| ["folder", "doc", "tag", "collection"].contains(&kind))
                && row["data"].is_string()
                && row["index"].is_string()
                && (row["parentId"].is_null() || row["parentId"].is_string()),
            "Unsupported folder row schema"
        );
        rows.push(row.clone());
    }
    Ok(rows)
}

fn persist(db: &Connection, id: &str, doc: &Doc, before: &StateVector) -> Result<bool> {
    let expected = table_json(doc)?;
    // A deletion changes the delete set, not necessarily the state vector.
    let previous = optional_doc(db, id)?
        .map(|stored| table_json(&stored))
        .transpose()?
        .unwrap_or_else(|| json!({}));
    if previous == expected {
        return Ok(false);
    }
    append_update(db, id, &doc.transact().encode_state_as_update_v1(before))?;
    ensure!(
        table_json(&load_doc(db, id)?)? == expected,
        "Organization readback failed: {id}"
    );
    Ok(true)
}

fn tombstone(doc: &Doc, id: &str, fields: &[&str]) {
    let row = doc.get_or_insert_map(id);
    let mut txn = doc.transact_mut();
    for field in fields {
        row.remove(&mut txn, field);
    }
    row.insert(&mut txn, "$$DELETED", true);
}

fn remove_folder_links(db: &Connection, kind: &str, id: &str) -> Result<bool> {
    let Some(folders) = optional_doc(db, "db$folders")? else {
        return Ok(false);
    };
    let before = folders.transact().state_vector();
    for row in folder_rows(&folders)? {
        if row["type"] == kind && row["data"] == id {
            tombstone(
                &folders,
                row["id"].as_str().unwrap(),
                &["parentId", "data", "type", "index"],
            );
        }
    }
    persist(db, "db$folders", &folders, &before)
}

fn mutation_result(
    workspace_id: &str,
    id_field: &str,
    id: &str,
    changed: bool,
    action: Action,
) -> Value {
    let mut result = json!({"workspaceId": workspace_id, "changed": changed, "deleted": action == Action::Delete});
    result[id_field] = json!(id);
    result
}

impl Storage {
    pub fn list_organization(
        &self,
        workspace_id: &str,
        kind: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        ensure!(
            (1..=200).contains(&limit),
            "limit must be between 1 and 200"
        );
        self.read(workspace_id, |db, _| {
            let root = load_doc(db, workspace_id)?;
            let mut items = match kind {
                "tags" => tags(&root)?,
                "collections" => collections(&root)?,
                "folders" => folder_rows(&optional_doc(db, "db$folders")?.unwrap_or_default())?,
                _ => bail!("Invalid organization kind"),
            };
            items.sort_by_key(|item| (item["index"].as_str().unwrap_or("").to_owned(), item["id"].as_str().unwrap().to_owned()));
            let total = items.len();
            let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
            let next = offset.saturating_add(items.len());
            let mut result = json!({"workspaceId": workspace_id, "total": total, "hasMore": next < total, "nextOffset": if next < total {Some(next)} else {None}});
            result[kind] = json!(items);
            Ok(result)
        })
    }

    pub fn manage_tag(&self, p: &ManageTag) -> Result<Value> {
        let id = entity_id(p.action, p.tag_id.as_deref(), p.confirm)?;
        if let Some(value) = &p.name {
            name(value)?;
        }
        if let Some(color) = &p.color {
            let theme = color.starts_with("--affine-tag-")
                && color.len() <= 80
                && color
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-');
            let hex = color.starts_with('#')
                && [4, 7, 9].contains(&color.len())
                && color[1..].bytes().all(|b| b.is_ascii_hexdigit());
            ensure!(
                theme || hex,
                "color must be an AFFiNE tag token or hex color"
            );
        }
        self.write(&p.workspace_id, |db| {
            let root = load_doc(db, &p.workspace_id)?;
            let before = root.transact().state_vector();
            let all = tags(&root)?;
            let position = all.iter().position(|tag| tag["id"] == id);
            let previous = position.map(|i| all[i].clone());
            let now = Utc::now().timestamp_millis();
            let mut related_changed = false;
            if p.action == Action::Delete {
                ensure!(p.name.is_none() && p.color.is_none(), "Delete does not accept name/color");
                if let Some(position) = position {
                    replace_record(&root, &tag_array(&root, false)?.unwrap(), Some(position), None, true)?;
                }
                // Repair stale assignments too when the option was already removed elsewhere.
                let meta = documents::root_json(&root, "meta")?;
                for entry in meta["pages"].as_array().context("Missing pages")? {
                    let current = entry["tags"].as_array().cloned().unwrap_or_default();
                    if current.iter().any(|tag| tag == &id) {
                        let page = document_entry(&root, &p.workspace_id, entry["id"].as_str().context("Missing document ID")?)?;
                        let remaining: Vec<_> = current.into_iter().filter(|tag| tag != &id).collect();
                        let array = nested_array(&root, &page, "tags", true)?.unwrap();
                        let mut txn = root.transact_mut();
                        let len = array.len(&txn);
                        array.remove_range(&mut txn, 0, len);
                        for tag in remaining {
                            array.push_back(&mut txn, serde_json::from_value::<yrs::Any>(tag)?);
                        }
                    }
                }
                related_changed = remove_folder_links(db, "tag", &id)?;
            } else {
                ensure!(p.name.is_some() || p.color.is_some(), "Supply name and/or color");
                ensure!(p.action != Action::Update || previous.is_some(), "Unknown tag");
                let mut value = previous.clone().unwrap_or_else(|| json!({"id":id,"value":"","color":"--affine-tag-blue","createDate":now,"updateDate":now}));
                if let Some(value_name) = &p.name { value["value"] = json!(name(value_name)?); }
                if let Some(color) = &p.color { value["color"] = json!(color); }
                name(value["value"].as_str().context("Tag name required on create")?)?;
                if p.action == Action::Create && previous.is_some() {
                    ensure!(previous == Some(value.clone()), "tagId already exists with different fields");
                } else if previous != Some(value.clone()) {
                    value["updateDate"] = json!(now);
                    let array = tag_array(&root, true)?.unwrap();
                    let native = position.and_then(|index| {
                        match array.get(&root.transact(), index as u32) {
                            Some(Out::YMap(map)) => Some(map),
                            _ => None,
                        }
                    });
                    if let Some(map) = native {
                        let mut txn = root.transact_mut();
                        for (key, requested) in [
                            ("value", p.name.is_some()),
                            ("color", p.color.is_some()),
                            ("updateDate", true),
                        ] {
                            if requested
                                && previous.as_ref().is_none_or(|record| record[key] != value[key])
                            {
                                map.insert(
                                    &mut txn,
                                    key,
                                    serde_json::from_value::<yrs::Any>(value[key].clone())?,
                                );
                            }
                        }
                    } else {
                        replace_record(&root, &array, position, Some(&value), true)?;
                    }
                }
            }
            let changed = persist(db, &p.workspace_id, &root, &before)? | related_changed;
            Ok(mutation_result(&p.workspace_id, "tagId", &id, changed, p.action))
        })
    }

    pub fn set_document_tags(&self, p: &SetDocumentTags) -> Result<Value> {
        ids(&p.tag_ids, 100)?;
        self.write(&p.workspace_id, |db| {
            let root = load_doc(db, &p.workspace_id)?;
            let before = root.transact().state_vector();
            let page = document_entry(&root, &p.workspace_id, &p.document_id)?;
            let meta = documents::root_json(&root, "meta")?;
            ensure!(active_pages(&meta)?.iter().any(|entry| entry["id"] == p.document_id), "Document is not active");
            let all = tags(&root)?;
            ensure!(p.tag_ids.iter().all(|id| all.iter().any(|tag| tag["id"] == *id)), "Unknown tag ID");
            let current = serde_json::to_value(page.to_json(&root.transact()))?;
            if current["tags"] != json!(p.tag_ids) {
                let array = nested_array(&root, &page, "tags", true)?.unwrap();
                let mut txn = root.transact_mut();
                let len = array.len(&txn);
                array.remove_range(&mut txn, 0, len);
                for id in &p.tag_ids { array.push_back(&mut txn, id.as_str()); }
            }
            let changed = persist(db, &p.workspace_id, &root, &before)?;
            let result = json!({"workspaceId": p.workspace_id, "documentId": p.document_id, "tagIds": p.tag_ids, "changed": changed});
            Ok(result)
        })
    }

    pub fn manage_collection(&self, p: &ManageCollection) -> Result<Value> {
        let id = entity_id(p.action, p.collection_id.as_deref(), p.confirm)?;
        ids(&p.add_document_ids, 200)?;
        ids(&p.remove_document_ids, 200)?;
        ensure!(
            !p.add_document_ids
                .iter()
                .any(|id| p.remove_document_ids.contains(id)),
            "Cannot add and remove the same document"
        );
        if let Some(value) = &p.name {
            name(value)?;
        }
        self.write(&p.workspace_id, |db| {
            let root = load_doc(db, &p.workspace_id)?;
            let before = root.transact().state_vector();
            let all = collections(&root)?;
            let position = all.iter().position(|item| item["id"] == id);
            let previous = position.map(|i| all[i].clone());
            let mut related_changed = false;
            if p.action == Action::Delete {
                ensure!(
                    p.name.is_none()
                        && p.add_document_ids.is_empty()
                        && p.remove_document_ids.is_empty(),
                    "Delete does not accept collection changes"
                );
                if let Some(position) = position {
                    replace_record(
                        &root,
                        &collection_array(&root, false)?.unwrap(),
                        Some(position),
                        None,
                        false,
                    )?;
                }
                related_changed = remove_folder_links(db, "collection", &id)?;
                if let Some(pinned) = optional_doc(db, "db$pinnedCollections")? {
                    let before = pinned.transact().state_vector();
                    let live = {
                        let txn = pinned.transact();
                        txn.get_map(id.as_str()).is_some_and(|row| {
                            row.get(&txn, "$$DELETED")
                                .is_none_or(|value| value.to_string(&txn) != "true")
                        })
                    };
                    if live {
                        tombstone(&pinned, &id, &["index"]);
                    }
                    related_changed |= persist(db, "db$pinnedCollections", &pinned, &before)?;
                }
            } else {
                ensure!(
                    p.action != Action::Update || previous.is_some(),
                    "Unknown collection"
                );
                let mut value = previous.clone().unwrap_or_else(
                    || json!({"id":id,"name":"","rules":{"filters":[]},"allowList":[]}),
                );
                if let Some(value_name) = &p.name {
                    value["name"] = json!(name(value_name)?);
                }
                name(
                    value["name"]
                        .as_str()
                        .context("Collection name required on create")?,
                )?;
                let mut members = value["allowList"]
                    .as_array()
                    .context("Unsupported collection allowList")?
                    .clone();
                members.retain(|id| !p.remove_document_ids.iter().any(|removed| id == removed));
                for id in &p.add_document_ids {
                    if !members.iter().any(|member| member == id) {
                        members.push(json!(id));
                    }
                }
                value["allowList"] = json!(members);
                // A matching retry does not add references, even if members are now trashed.
                if p.action == Action::Create && previous.is_some() {
                    ensure!(
                        previous == Some(value.clone()),
                        "collectionId already exists with different fields"
                    );
                } else {
                    let meta = documents::root_json(&root, "meta")?;
                    let pages = active_pages(&meta)?;
                    ensure!(
                        p.add_document_ids
                            .iter()
                            .all(|id| pages.iter().any(|page| page["id"] == *id)),
                        "Can only add active documents"
                    );
                    if previous != Some(value.clone()) {
                        replace_record(
                            &root,
                            &collection_array(&root, true)?.unwrap(),
                            position,
                            Some(&value),
                            false,
                        )?;
                    }
                }
            }
            let changed = persist(db, &p.workspace_id, &root, &before)? | related_changed;
            Ok(mutation_result(
                &p.workspace_id,
                "collectionId",
                &id,
                changed,
                p.action,
            ))
        })
    }

    pub fn manage_folder(&self, p: &ManageFolder) -> Result<Value> {
        let id = entity_id(p.action, p.node_id.as_deref(), p.confirm)?;
        self.write(&p.workspace_id, |db| {
            let doc = optional_doc(db, "db$folders")?.unwrap_or_default();
            let before = doc.transact().state_vector();
            let all = folder_rows(&doc)?;
            let previous = all.iter().find(|row| row["id"] == id).cloned();
            if p.action == Action::Delete {
                ensure!(
                    p.kind.is_none()
                        && p.name.is_none()
                        && p.target_id.is_none()
                        && p.parent_id.is_none()
                        && p.index.is_none(),
                    "Delete only accepts nodeId and confirm"
                );
                let mut deleting = HashSet::from([id.clone()]);
                loop {
                    let old_len = deleting.len();
                    for row in &all {
                        if row["parentId"]
                            .as_str()
                            .is_some_and(|parent| deleting.contains(parent))
                        {
                            deleting.insert(row["id"].as_str().unwrap().to_owned());
                        }
                    }
                    if old_len == deleting.len() {
                        break;
                    }
                }
                for row in &all {
                    if deleting.contains(row["id"].as_str().unwrap()) {
                        tombstone(
                            &doc,
                            row["id"].as_str().unwrap(),
                            &["parentId", "data", "type", "index"],
                        );
                    }
                }
            } else {
                ensure!(
                    p.action != Action::Update || previous.is_some(),
                    "Unknown folder node"
                );
                let kind = match (&previous, p.kind) {
                    (Some(row), None) => row["type"].as_str().context("Invalid node type")?,
                    (_, Some(kind)) => kind.as_str(),
                    _ => bail!("kind required on create"),
                };
                if let Some(row) = &previous {
                    ensure!(row["type"] == kind, "Cannot change node kind");
                }
                let mut value = previous.clone().unwrap_or_else(
                    || json!({"id":id,"type":kind,"parentId":null,"data":"","index":""}),
                );
                if kind == "folder" {
                    ensure!(p.target_id.is_none(), "Folders use name, not targetId");
                    if let Some(value_name) = &p.name {
                        value["data"] = json!(name(value_name)?);
                    }
                    name(value["data"].as_str().context("Folder name required")?)?;
                } else {
                    ensure!(p.name.is_none(), "Shortcuts use targetId, not name");
                    if let Some(target) = &p.target_id {
                        value["data"] = json!(target);
                    }
                    let target = value["data"].as_str().context("targetId required")?;
                    ensure!(identifier(target), "Invalid targetId");
                }
                if let Some(parent) = &p.parent_id {
                    value["parentId"] = if parent.is_empty() {
                        Value::Null
                    } else {
                        json!(parent)
                    };
                }
                let parent = value["parentId"].as_str();
                ensure!(
                    parent.is_some() || kind == "folder",
                    "Root nodes must be folders"
                );
                let mut seen = HashSet::from([id.as_str()]);
                let mut cursor = parent;
                while let Some(parent) = cursor {
                    ensure!(seen.insert(parent), "Folder cycle detected");
                    let row = all
                        .iter()
                        .find(|row| row["id"] == parent && row["type"] == "folder")
                        .context("Parent folder not found")?;
                    cursor = row["parentId"].as_str();
                }
                let moved = previous
                    .as_ref()
                    .is_some_and(|row| row["parentId"] != value["parentId"]);
                if let Some(index) = &p.index {
                    canvas::next_index(Some(index))?;
                    value["index"] = json!(index);
                } else if previous.is_none() || moved {
                    let last = all
                        .iter()
                        .filter(|row| row["parentId"] == value["parentId"] && row["id"] != id)
                        .filter_map(|row| row["index"].as_str())
                        .max();
                    value["index"] = json!(canvas::next_index(last)?);
                }
                // Validate active targets only after recognizing an unchanged create retry.
                if p.action == Action::Create && previous.is_some() {
                    ensure!(
                        previous == Some(value.clone()),
                        "nodeId already exists with different fields"
                    );
                } else {
                    if kind != "folder" {
                        let target = value["data"].as_str().context("targetId required")?;
                        let root = load_doc(db, &p.workspace_id)?;
                        let exists = match kind {
                            "doc" => active_pages(&documents::root_json(&root, "meta")?)?
                                .iter()
                                .any(|page| page["id"] == target),
                            "tag" => tags(&root)?.iter().any(|tag| tag["id"] == target),
                            "collection" => {
                                collections(&root)?.iter().any(|item| item["id"] == target)
                            }
                            _ => bail!("Unsupported folder node type"),
                        };
                        ensure!(exists, "Unknown/inactive shortcut target");
                    }
                    if previous != Some(value.clone()) {
                        let map = doc.get_or_insert_map(id.as_str());
                        let mut txn = doc.transact_mut();
                        for key in ["id", "type", "parentId", "data", "index"] {
                            map.insert(
                                &mut txn,
                                key,
                                serde_json::from_value::<yrs::Any>(value[key].clone())?,
                            );
                        }
                        map.remove(&mut txn, "$$DELETED");
                    }
                }
            }
            let changed = persist(db, "db$folders", &doc, &before)?;
            Ok(mutation_result(
                &p.workspace_id,
                "nodeId",
                &id,
                changed,
                p.action,
            ))
        })
    }
}
