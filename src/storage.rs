use crate::{
    canvas,
    documents::{self, Rendered},
    drawing, media,
};
use anyhow::{Context, Result, bail, ensure};
use chrono::{Duration as ChronoDuration, NaiveDateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;
use yrs::{
    Array, ArrayPrelim, Doc, In, Map, MapPrelim, Out, ReadTxn, TextPrelim, Transact, Update,
    types::ToJson, updates::decoder::Decode,
};

#[derive(Clone)]
pub struct Storage {
    pub data_dir: PathBuf,
    pub allowlist: Option<HashSet<String>>,
    pub writes: bool,
    write_lock: Arc<Mutex<()>>,
}

#[derive(Clone)]
struct Workspace {
    id: String,
    path: PathBuf,
}

struct NewDocument<'a> {
    id: &'a str,
    title: &'a str,
    mode: &'a str,
    document: Doc,
    plain_text: Option<&'a str>,
}

pub fn data_dir_for(
    platform: &str,
    home: &Path,
    appdata: Option<PathBuf>,
    xdg: Option<PathBuf>,
) -> PathBuf {
    match platform {
        "macos" => home.join("Library/Application Support/AFFiNE"),
        "windows" => appdata
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("AFFiNE"),
        _ => xdg.unwrap_or_else(|| home.join(".config")).join("AFFiNE"),
    }
}

pub(crate) fn identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn ensure_affine_closed() -> Result<()> {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let running = system.processes().values().any(|process| {
        let name = process.name().to_string_lossy().to_ascii_lowercase();
        name == "affine" || name == "affine.exe" || name.starts_with("affine helper")
    });
    ensure!(
        !running,
        "Quit AFFiNE Desktop before writing. Offline writes cannot notify its in-memory event channel; reopen AFFiNE after changes."
    );
    Ok(())
}

impl Storage {
    pub fn new(data_dir: PathBuf, writes: bool) -> Self {
        Self {
            data_dir,
            writes,
            allowlist: None,
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn from_env() -> Result<Self> {
        let home = dirs::home_dir().context("Cannot find home directory; set AFFINE_DATA_DIR")?;
        let path = env::var_os("AFFINE_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                data_dir_for(
                    env::consts::OS,
                    &home,
                    env::var_os("APPDATA").map(PathBuf::from),
                    env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
                )
            });
        let mut storage = Self::new(path, env::var("AFFINE_ENABLE_WRITES").as_deref() == Ok("1"));
        if let Ok(ids) = env::var("AFFINE_WORKSPACE_IDS") {
            let ids: HashSet<_> = ids
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            ensure!(
                ids.iter().all(|id| identifier(id)),
                "Invalid AFFINE_WORKSPACE_IDS"
            );
            storage.allowlist = Some(ids);
        }
        Ok(storage)
    }

    fn discover(&self) -> Result<Vec<Workspace>> {
        let root = self.data_dir.join("workspaces/local");
        let real_root = root.canonicalize().with_context(|| format!("No local workspaces at {}. Set AFFINE_DATA_DIR to AFFiNE's application data directory.", root.display()))?;
        let mut result = Vec::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if !entry.file_type()?.is_dir() || !identifier(&id) {
                continue;
            }
            if self
                .allowlist
                .as_ref()
                .is_some_and(|ids| !ids.contains(&id))
            {
                continue;
            }
            let path = entry.path().join("storage.db");
            if !path.is_file() {
                continue;
            }
            let path = path.canonicalize()?;
            if path.starts_with(&real_root) {
                result.push(Workspace { id, path });
            }
        }
        result.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(result)
    }

    fn workspace(&self, id: &str) -> Result<Workspace> {
        self.discover()?
            .into_iter()
            .find(|ws| ws.id == id)
            .with_context(|| format!("Unknown or disallowed local workspace: {id}"))
    }

    fn open(&self, id: &str, write: bool) -> Result<Connection> {
        let workspace = self.workspace(id)?;
        let flags = if write {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        let db = Connection::open_with_flags(workspace.path, flags)?;
        db.busy_timeout(Duration::from_secs(3))?;
        if !write {
            db.execute_batch("PRAGMA query_only=ON;")?;
        }
        validate_schema(&db)?;
        let space_id: String = db.query_row("SELECT space_id FROM meta", [], |row| row.get(0))?;
        ensure!(
            space_id == id,
            "Workspace directory and SQLite space_id disagree"
        );
        Ok(db)
    }

    fn read<T>(
        &self,
        id: &str,
        operation: impl FnOnce(&Connection, &Value) -> Result<T>,
    ) -> Result<T> {
        let mut db = self.open(id, false)?;
        let tx = db.transaction()?;
        let meta = registry(&tx, id)?;
        operation(&tx, &meta)
    }

    pub fn list_workspaces(&self) -> Result<Value> {
        let mut workspaces = Vec::new();
        let mut warnings = Vec::new();
        for workspace in self.discover()? {
            match self.read(&workspace.id, |_, meta| Ok(json!({
                "id": workspace.id, "name": meta["name"], "documentCount": active_pages(meta)?.len()
            }))) {
                Ok(workspace) => workspaces.push(workspace),
                Err(error) => warnings.push(json!({"workspaceId": workspace.id, "error": error.to_string()})),
            }
        }
        Ok(json!({"workspaces": workspaces, "warnings": warnings, "writesEnabled": self.writes}))
    }

    pub fn list_documents(&self, workspace_id: &str, offset: usize, limit: usize) -> Result<Value> {
        ensure!(
            (1..=200).contains(&limit),
            "limit must be between 1 and 200"
        );
        self.read(workspace_id, |db, meta| {
            let pages = active_pages(meta)?;
            let properties = optional_doc(db, "db$docProperties")?;
            let documents: Vec<_> = pages.iter().skip(offset).take(limit).map(|page| {
                let id = page["id"].as_str().unwrap();
                let mode = mode_for(properties.as_ref(), id).unwrap_or_else(|_| "unknown".into());
                json!({"id": id, "title": page["title"].as_str().filter(|s| !s.is_empty()).unwrap_or("Untitled"), "mode": mode, "createdAt": page["createDate"], "updatedAt": page["updatedDate"]})
            }).collect();
            let next = offset.saturating_add(documents.len());
            Ok(json!({"workspaceId": workspace_id, "documents": documents, "total": pages.len(), "hasMore": next < pages.len(), "nextOffset": if next < pages.len() {Some(next)} else {None}}))
        })
    }

    pub fn read_document(
        &self,
        workspace_id: &str,
        document_id: &str,
        offset: usize,
        max_chars: usize,
    ) -> Result<Value> {
        ensure!(
            (1..=100_000).contains(&max_chars),
            "maxChars must be between 1 and 100000"
        );
        self.read(workspace_id, |db, meta| {
            let rendered = active_document(db, meta, document_id)?;
            let chars: Vec<_> = rendered.markdown.chars().collect();
            let markdown: String = chars.iter().skip(offset).take(max_chars).collect();
            let next = offset.saturating_add(markdown.chars().count());
            let properties = optional_doc(db, "db$docProperties")?;
            Ok(json!({"workspaceId": workspace_id, "documentId": document_id, "title": rendered.title, "mode": mode_for(properties.as_ref(), document_id)?, "markdown": markdown, "offset": offset, "totalChars": chars.len(), "hasMore": next < chars.len(), "nextOffset": if next < chars.len() {Some(next)} else {None}, "blockCount": rendered.block_count, "warnings": rendered.warnings}))
        })
    }

    pub fn search(
        &self,
        workspace_id: &str,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        ensure!(
            (1..=100).contains(&limit),
            "limit must be between 1 and 100"
        );
        ensure!(
            !query.trim().is_empty() && query.len() <= 1000,
            "query must contain 1..1000 bytes of nonblank text"
        );
        self.read(workspace_id, |db, meta| {
            let needle = query.trim().to_lowercase();
            let mut matches = Vec::new();
            let mut warnings = Vec::new();
            for page in active_pages(meta)? {
                let id = page["id"].as_str().unwrap();
                match active_document(db, meta, id) {
                    Ok(doc) => {
                        let title_match = doc.title.to_lowercase().contains(&needle);
                        if title_match || doc.markdown.to_lowercase().contains(&needle) {
                            // A bounded preview; Unicode case-folding need not preserve byte offsets.
                            let snippet: String = doc.markdown.chars().take(400).collect();
                            matches.push(json!({"documentId": id, "title": doc.title, "titleMatch": title_match, "snippet": snippet}));
                        }
                    }
                    Err(error) => warnings.push(json!({"documentId": id, "error": error.to_string()})),
                }
            }
            matches.sort_by_key(|item| (!item["titleMatch"].as_bool().unwrap_or(false), item["documentId"].as_str().unwrap_or("").to_owned()));
            let total = matches.len();
            let results: Vec<_> = matches.into_iter().skip(offset).take(limit).collect();
            let next = offset.saturating_add(results.len());
            Ok(json!({"workspaceId": workspace_id, "query": query, "results": results, "total": total, "hasMore": next < total, "nextOffset": if next < total {Some(next)} else {None}, "partial": !warnings.is_empty(), "warnings": warnings}))
        })
    }

    pub fn create(
        &self,
        workspace_id: &str,
        title: &str,
        content: &str,
        mode: &str,
        requested_id: Option<&str>,
    ) -> Result<Value> {
        ensure!(
            !title.trim().is_empty() && title.chars().count() <= 500,
            "title must contain 1..500 characters"
        );
        ensure!(
            content.len() <= 100_000,
            "content must be at most 100000 UTF-8 bytes"
        );
        ensure!(
            mode == "page" || mode == "edgeless",
            "mode must be page or edgeless"
        );
        let id = requested_id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        ensure!(
            identifier(&id),
            "documentId must contain 1..128 ASCII letters, digits, underscores or hyphens"
        );
        self.save_new_document(
            workspace_id,
            NewDocument {
                id: &id,
                title: title.trim(),
                mode,
                document: build_document(&id, title.trim(), content, mode)?,
                plain_text: Some(content),
            },
        )
    }

    fn write(
        &self,
        workspace_id: &str,
        operation: impl FnOnce(&Connection) -> Result<Value>,
    ) -> Result<Value> {
        ensure!(
            self.writes,
            "Writes disabled. Set AFFINE_ENABLE_WRITES=1 and quit AFFiNE Desktop before writing."
        );
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Write lock poisoned"))?;
        ensure_affine_closed()?;
        let mut db = self.open(workspace_id, true)?;
        // Online SQLite backup includes WAL content; a file copy would not.
        let backup_dir = self.data_dir.join("mcp-backups");
        fs::create_dir_all(&backup_dir)?;
        let backup = backup_dir.join(format!("{workspace_id}-{}.db", Uuid::new_v4()));
        db.backup("main", &backup, None)?;
        ensure_affine_closed()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut result = operation(&tx)?;
        let result_fields = result.as_object_mut().context("Invalid write result")?;
        result_fields.insert("backupPath".into(), json!(backup));
        result_fields.insert("reopenAffine".into(), json!(true));
        ensure_affine_closed()?;
        tx.commit()?;
        Ok(result)
    }

    fn save_new_document(&self, workspace_id: &str, prepared: NewDocument<'_>) -> Result<Value> {
        let NewDocument {
            id,
            title,
            mode,
            document,
            plain_text,
        } = prepared;
        self.write(workspace_id, |tx| {
        let root = load_doc(tx, workspace_id)?;
        let meta = documents::root_json(&root, "meta")?;
        if active_pages(&meta)?.iter().any(|page| page["id"] == id) {
            let existing = active_document(tx, &meta, id)?;
            let properties = optional_doc(tx, "db$docProperties")?;
            let content_matches = if let Some(content) = plain_text {
                existing.markdown == if content.is_empty() {
                    format!("# {title}")
                } else {
                    format!("# {title}\n\n{content}")
                }
            } else {
                documents::root_json(&load_doc(tx, id)?, "blocks")?
                    == documents::root_json(&document, "blocks")?
            };
            ensure!(
                existing.title == title
                    && content_matches
                    && mode_for(properties.as_ref(), id)? == mode,
                "documentId already exists with different content or mode; nothing was changed"
            );
            return Ok(
                json!({"documentId": id, "workspaceId": workspace_id, "mode": mode, "alreadyExists": true}),
            );
        }
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM snapshots WHERE doc_id=?1 UNION SELECT 1 FROM updates WHERE doc_id=?1)", [&id], |row| row.get(0))?;
        ensure!(
            !exists,
            "documentId belongs to an inactive/internal document; choose a new ID"
        );
        let now = Utc::now().timestamp_millis();

        let document_update = document
            .transact()
            .encode_state_as_update_v1(&Default::default());
        let before = root.transact().state_vector();
        {
            let mut txn = root.transact_mut();
            let meta = txn.get_map("meta").context("Missing metadata")?;
            let Some(Out::YArray(pages)) = meta.get(&txn, "pages") else {
                bail!("Unsupported registry: pages must be a Y.Array")
            };
            let page = MapPrelim::from([
                ("id", In::from(id)),
                ("title", In::from(title)),
                ("createDate", In::from(now as f64)),
                ("updatedDate", In::from(now as f64)),
                ("tags", In::from(ArrayPrelim::default())),
            ]);
            pages.push_back(&mut txn, page);
        }
        let root_update = root.transact().encode_state_as_update_v1(&before);
        let properties = optional_doc(tx, "db$docProperties")?.unwrap_or_default();
        let property_before = properties.transact().state_vector();
        let property_map = properties.get_or_insert_map(id);
        {
            let mut txn = properties.transact_mut();
            property_map.insert(&mut txn, "id", id);
            property_map.insert(&mut txn, "primaryMode", mode);
        }
        let property_update = properties
            .transact()
            .encode_state_as_update_v1(&property_before);
        append_update(tx, id, &document_update)?;
        append_update(tx, "db$docProperties", &property_update)?;
        append_update(tx, workspace_id, &root_update)?;
        // Independent readback before commit validates all dependent writes together.
        let saved_meta = registry(tx, workspace_id)?;
        let saved = active_document(tx, &saved_meta, id)?;
        ensure!(
            saved.title == title && documents::root_json(&load_doc(tx, id)?, "blocks")? == documents::root_json(&document, "blocks")?,
            "Created document readback failed"
        );
        Ok(
            json!({"documentId": id, "workspaceId": workspace_id, "title": title, "mode": mode, "alreadyExists": false}),
        )
        })
    }

    pub fn create_diagram(&self, params: &canvas::CreateDiagram) -> Result<Value> {
        ensure!(
            !params.title.trim().is_empty() && params.title.chars().count() <= 500,
            "title must contain 1..500 characters"
        );
        let id = params
            .document_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        ensure!(identifier(&id), "Invalid documentId");
        let bounds = canvas::layout(&params.nodes, &params.edges, params.direction)?;
        let document = build_document(&id, params.title.trim(), "", "edgeless")?;
        let elements = canvas::elements(&document)?;
        let by_id: HashMap<_, _> = params
            .nodes
            .iter()
            .zip(&bounds)
            .map(|(n, b)| (n.id.as_str(), *b))
            .collect();
        for (node, bounds) in params.nodes.iter().zip(bounds) {
            ensure!(
                !document
                    .transact()
                    .get_map("blocks")
                    .unwrap()
                    .contains_key(&document.transact(), &node.id),
                "Shape ID conflicts with a block ID"
            );
            canvas::upsert(
                &document,
                &elements,
                &node.id,
                canvas::shape_props(node, bounds),
            )?;
        }
        for edge in &params.edges {
            ensure!(
                !document
                    .transact()
                    .get_map("blocks")
                    .unwrap()
                    .contains_key(&document.transact(), &edge.id),
                "Connector ID conflicts with a block ID"
            );
            canvas::upsert(
                &document,
                &elements,
                &edge.id,
                canvas::edge_props(
                    edge,
                    by_id[edge.source.as_str()],
                    by_id[edge.target.as_str()],
                ),
            )?;
        }
        self.save_new_document(
            &params.workspace_id,
            NewDocument {
                id: &id,
                title: params.title.trim(),
                mode: "edgeless",
                document,
                plain_text: None,
            },
        )
    }

    pub fn read_canvas(&self, workspace_id: &str, document_id: &str) -> Result<Value> {
        self.read(workspace_id, |db, meta| {
            active_document(db, meta, document_id)?;
            let document = load_doc(db, document_id)?;
            let elements = canvas::elements(&document)?;
            let raw = serde_json::to_value(elements.to_json(&document.transact()))?;
            let mut elements: Vec<_> = raw.as_object().context("Invalid surface elements")?.iter().map(|(id, value)| json!({"id":id,"properties":value})).collect();
            elements.sort_by_key(|value| (value["properties"]["index"].as_str().unwrap_or("").to_owned(), value["id"].as_str().unwrap().to_owned()));
            let blocks = drawing::canvas_blocks(&document)?;
            Ok(json!({"workspaceId":workspace_id,"documentId":document_id,"elementCount":elements.len(),"elements":elements,"canvasBlockCount":blocks.len(),"blocks":blocks}))
        })
    }

    pub fn edit_canvas(&self, params: &canvas::EditCanvas) -> Result<Value> {
        drawing::validate(params)?;
        self.mutate_canvas(
            &params.workspace_id,
            &params.document_id,
            |document, db, meta| {
                for link in &params.links {
                    if let drawing::LinkTarget::Document { document_id } = &link.target {
                        active_document(db, meta, document_id)?;
                    }
                }
                let prepared = media::prepare(&self.data_dir, &params.media)?;
                media::persist(db, &prepared)?;
                drawing::apply(document, params, &prepared)
            },
        )
    }

    pub fn erase_canvas(&self, params: &drawing::EraseCanvas) -> Result<Value> {
        self.mutate_canvas(
            &params.workspace_id,
            &params.document_id,
            |document, _, _| drawing::erase(document, params),
        )
    }

    /// One guarded, backed-up transaction for primitives, block references, blobs,
    /// and workspace registry updates, with independent persisted-state readback.
    fn mutate_canvas(
        &self,
        workspace_id: &str,
        document_id: &str,
        operation: impl FnOnce(&Doc, &Connection, &Value) -> Result<Value>,
    ) -> Result<Value> {
        self.write(workspace_id, |db| {
            let root = load_doc(db, workspace_id)?;
            let meta = documents::root_json(&root, "meta")?;
            active_document(db, &meta, document_id)?;
            let document = load_doc(db, document_id)?;
            let before = document.transact().state_vector();
            let mut result = operation(&document, db, &meta)?;
            let expected = documents::root_json(&document, "blocks")?;
            append_update(
                db,
                document_id,
                &document.transact().encode_state_as_update_v1(&before),
            )?;
            let root_before = root.transact().state_vector();
            {
                let mut txn = root.transact_mut();
                let meta = txn.get_map("meta").context("Missing metadata")?;
                let Some(Out::YArray(pages)) = meta.get(&txn, "pages") else {
                    bail!("Unsupported registry");
                };
                let page = pages
                    .iter(&txn)
                    .find_map(|value| {
                        if let Out::YMap(page) = value
                            && page
                                .get(&txn, "id")
                                .is_some_and(|id| id.to_string(&txn) == document_id)
                        {
                            Some(page)
                        } else {
                            None
                        }
                    })
                    .context("Missing document registry entry")?;
                page.insert(
                    &mut txn,
                    "updatedDate",
                    Utc::now().timestamp_millis() as f64,
                );
            }
            append_update(
                db,
                workspace_id,
                &root.transact().encode_state_as_update_v1(&root_before),
            )?;
            let saved = load_doc(db, document_id)?;
            ensure!(
                documents::root_json(&saved, "blocks")? == expected,
                "Native canvas/block readback failed"
            );
            let raw = serde_json::to_value(canvas::elements(&saved)?.to_json(&saved.transact()))?;
            result["workspaceId"] = json!(workspace_id);
            result["documentId"] = json!(document_id);
            result["elementCount"] = json!(raw.as_object().context("Invalid saved surface")?.len());
            result["canvasBlockCount"] = json!(drawing::canvas_blocks(&saved)?.len());
            Ok(result)
        })
    }
}

fn validate_schema(db: &Connection) -> Result<()> {
    for (table, required) in [
        ("meta", vec!["space_id"]),
        ("snapshots", vec!["doc_id", "data", "updated_at"]),
        ("updates", vec!["doc_id", "data", "created_at"]),
        ("clocks", vec!["doc_id", "timestamp"]),
    ] {
        let mut stmt = db.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<HashSet<_>>>()?;
        ensure!(
            required.iter().all(|column| columns.contains(*column)),
            "Unsupported AFFiNE storage schema: {table}"
        );
    }
    Ok(())
}

fn optional_doc(db: &Connection, id: &str) -> Result<Option<Doc>> {
    let snapshot: Option<Vec<u8>> = db
        .query_row("SELECT data FROM snapshots WHERE doc_id=?", [id], |row| {
            row.get(0)
        })
        .optional()?;
    let mut statement =
        db.prepare("SELECT data FROM updates WHERE doc_id=? ORDER BY created_at")?;
    let updates = statement
        .query_map([id], |row| row.get::<_, Vec<u8>>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if snapshot.is_none() && updates.is_empty() {
        return Ok(None);
    }
    let doc = Doc::new();
    {
        let mut txn = doc.transact_mut();
        for bytes in snapshot.into_iter().chain(updates) {
            txn.apply_update(
                Update::decode_v1(&bytes).with_context(|| format!("Invalid Yjs data for {id}"))?,
            )?;
        }
        ensure!(
            !txn.has_missing_updates(),
            "Incomplete Yjs data for {id}; let AFFiNE finish persisting/syncing it"
        );
    }
    Ok(Some(doc))
}

fn load_doc(db: &Connection, id: &str) -> Result<Doc> {
    optional_doc(db, id)?.with_context(|| format!("Document not stored locally: {id}"))
}

fn registry(db: &Connection, id: &str) -> Result<Value> {
    let meta = documents::root_json(&load_doc(db, id)?, "meta")?;
    active_pages(&meta)?;
    Ok(meta)
}

fn active_pages(meta: &Value) -> Result<Vec<&Value>> {
    let pages = meta["pages"]
        .as_array()
        .context("Unsupported workspace: missing active document registry")?;
    let mut seen = HashSet::new();
    Ok(pages
        .iter()
        .filter(|page| {
            page["id"].as_str().is_some_and(|id| seen.insert(id))
                && !page["trash"].as_bool().unwrap_or(false)
        })
        .collect())
}

fn active_document(db: &Connection, meta: &Value, id: &str) -> Result<Rendered> {
    ensure!(
        active_pages(meta)?.iter().any(|page| page["id"] == id),
        "Document is not active in this workspace: {id}"
    );
    documents::render(&load_doc(db, id)?)
}

fn mode_for(properties: Option<&Doc>, id: &str) -> Result<String> {
    let Some(properties) = properties else {
        return Ok("page".into());
    };
    let txn = properties.transact();
    let Some(map) = txn.get_map(id) else {
        return Ok("page".into());
    };
    Ok(map
        .get(&txn, "primaryMode")
        .map(|value| value.to_string(&txn))
        .unwrap_or_else(|| "page".into()))
}

fn append_update(db: &Connection, id: &str, bytes: &[u8]) -> Result<()> {
    let latest: Option<String> = db.query_row(
        "SELECT max(timestamp) FROM clocks WHERE doc_id=?",
        [id],
        |row| row.get(0),
    )?;
    let mut timestamp = Utc::now().naive_utc();
    if let Some(latest) = latest {
        let previous = NaiveDateTime::parse_from_str(&latest, "%Y-%m-%d %H:%M:%S%.f")
            .context("Unsupported SQLite clock timestamp")?;
        timestamp = timestamp.max(previous + ChronoDuration::milliseconds(1));
    }
    let timestamp = timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
    // Matches nbstore::SqliteDocStorage::try_insert_update_with_timestamp in AFFiNE v0.27.4.
    db.execute(
        "INSERT INTO updates(doc_id,data,created_at) VALUES (?1,?2,?3)",
        params![id, bytes, timestamp],
    )?;
    db.execute("INSERT INTO clocks(doc_id,timestamp) VALUES (?1,?2) ON CONFLICT(doc_id) DO UPDATE SET timestamp=excluded.timestamp", params![id, timestamp])?;
    Ok(())
}

pub fn build_document(id: &str, title: &str, content: &str, mode: &str) -> Result<Doc> {
    let doc = Doc::new();
    let blocks = doc.get_or_insert_map("blocks");
    let page_id = format!("{id}-page");
    let surface_id = format!("{id}-surface");
    let note_id = format!("{id}-note");
    let text_id = format!("{id}-text");
    let include_note = mode != "edgeless" || !content.is_empty();
    {
        let mut txn = doc.transact_mut();
        let mut block = |id: &str, flavour: &str, version: i32, children: Vec<String>| {
            blocks.insert(
                &mut txn,
                id,
                MapPrelim::from([
                    ("sys:id", In::from(id)),
                    ("sys:flavour", In::from(flavour)),
                    ("sys:version", In::from(version)),
                    ("sys:children", In::from(ArrayPrelim::from(children))),
                ]),
            )
        };
        let page = block(
            &page_id,
            "affine:page",
            2,
            if include_note {
                vec![surface_id.clone(), note_id.clone()]
            } else {
                vec![surface_id.clone()]
            },
        );
        let surface = block(&surface_id, "affine:surface", 5, vec![]);
        let note_blocks = if include_note {
            Some((
                block(&note_id, "affine:note", 1, vec![text_id.clone()]),
                block(&text_id, "affine:paragraph", 1, vec![]),
            ))
        } else {
            None
        };
        page.insert(&mut txn, "prop:title", TextPrelim::new(title));
        surface.insert(
            &mut txn,
            "prop:elements",
            MapPrelim::from([
                ("type", In::from("$blocksuite:internal:native$")),
                ("value", In::from(MapPrelim::default())),
            ]),
        );
        if let Some((note, paragraph)) = note_blocks {
            note.insert(&mut txn, "prop:xywh", "[100,100,800,300]");
            note.insert(&mut txn, "prop:index", "a0");
            note.insert(&mut txn, "prop:lockedBySelf", false);
            note.insert(&mut txn, "prop:hidden", false);
            note.insert(&mut txn, "prop:displayMode", "both");
            note.insert(
                &mut txn,
                "prop:background",
                serde_json::from_value::<yrs::Any>(json!({"light":"#ffffff", "dark":"#252525"}))?,
            );
            note.insert(&mut txn, "prop:edgeless", serde_json::from_value::<yrs::Any>(json!({"style":{"borderRadius":8,"borderSize":4,"borderStyle":"none","shadowType":"--affine-note-shadow-box"},"collapse":false}))?);
            paragraph.insert(&mut txn, "prop:type", "text");
            paragraph.insert(&mut txn, "prop:text", TextPrelim::new(content));
            paragraph.insert(&mut txn, "prop:collapsed", false);
        }
    }
    ensure!(
        mode == "page" || mode == "edgeless",
        "Unsupported document mode"
    );
    Ok(doc)
}
