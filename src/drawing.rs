//! AFFiNE v0.27.4 brush primitives and native canvas blocks.
use crate::{
    canvas::{self, Color, Edge, EditCanvas, Routing},
    documents, media,
    storage::identifier,
};
use anyhow::{Context, Result, bail, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value, json};
use std::collections::{HashMap, HashSet};
use yrs::{
    Array, ArrayPrelim, Doc, Map, MapPrelim, Out, ReadTxn, Text, TextPrelim, Transact,
    types::ToJson,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stroke {
    pub id: String,
    /// Absolute canvas [x,y] or [x,y,pressure] points. Pressure must be 0..1.
    pub points: Vec<Vec<f64>>,
    #[serde(default = "default_line_width")]
    pub line_width: f64,
    pub color: Option<Color>,
}
fn default_line_width() -> f64 {
    4.0
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DrawPen {
    pub workspace_id: String,
    pub document_id: String,
    pub strokes: Vec<Stroke>,
}
impl DrawPen {
    pub fn into_edit(self) -> EditCanvas {
        EditCanvas {
            workspace_id: self.workspace_id,
            document_id: self.document_id,
            strokes: self.strokes,
            ..Default::default()
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Curve {
    pub id: String,
    /// Absolute, unbound endpoints. AFFiNE computes the editable native curve.
    pub source: [f64; 2],
    pub target: [f64; 2],
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub start_arrow: bool,
    #[serde(default)]
    pub end_arrow: bool,
    pub stroke: Option<Color>,
    pub stroke_width: Option<f64>,
    pub label_color: Option<Color>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Frame {
    pub id: String,
    pub title: String,
    pub bounds: [f64; 4],
    /// Omit to preserve existing membership; [] explicitly empties the frame.
    /// Members remain editable native objects, not rasterized frame contents.
    pub child_ids: Option<Vec<String>>,
    pub background: Option<Color>,
    pub presentation_index: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum LinkTarget {
    External {
        url: String,
    },
    Document {
        #[serde(rename = "documentId")]
        document_id: String,
    },
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Link {
    pub id: String,
    pub target: LinkTarget,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub position: [f64; 2],
    pub size: Option<[f64; 2]>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EraseCanvas {
    pub workspace_id: String,
    pub document_id: String,
    /// Explicit brush IDs. Other object types use affine_edit_canvas.deleteIds.
    #[serde(default)]
    pub stroke_ids: Vec<String>,
    /// Swept path in absolute canvas coordinates. Erases entire intersecting brush
    /// strokes, like AFFiNE's object eraser, not pixels or partial stroke segments.
    #[serde(default)]
    pub path: Vec<[f64; 2]>,
    #[serde(default = "default_eraser_radius")]
    pub radius: f64,
    #[serde(default)]
    pub confirm: bool,
}
fn default_eraser_radius() -> f64 {
    8.0
}

pub(crate) fn validate_position(p: [f64; 2]) -> Result<()> {
    ensure!(
        p.iter().all(|n| n.is_finite() && n.abs() <= 1_000_000.0),
        "Canvas coordinates must be finite and within +/-1,000,000"
    );
    Ok(())
}
pub(crate) fn validate_size(size: [f64; 2]) -> Result<()> {
    ensure!(
        size.iter()
            .all(|n| n.is_finite() && *n > 0.0 && *n <= 20_000.0),
        "Canvas width/height must be finite and in (0,20000]"
    );
    Ok(())
}

pub(crate) fn ids(p: &EditCanvas) -> Vec<&str> {
    p.nodes
        .iter()
        .map(|x| x.id.as_str())
        .chain(p.edges.iter().map(|x| x.id.as_str()))
        .chain(p.strokes.iter().map(|x| x.id.as_str()))
        .chain(p.curves.iter().map(|x| x.id.as_str()))
        .chain(p.frames.iter().map(|x| x.id.as_str()))
        .chain(p.links.iter().map(|x| x.id.as_str()))
        .chain(p.media.iter().map(|x| x.id.as_str()))
        .collect()
}

pub(crate) fn validate(p: &EditCanvas) -> Result<()> {
    ensure!(identifier(&p.document_id), "Invalid document ID");
    let upserted = ids(p);
    ensure!(
        !upserted.is_empty() || !p.delete_ids.is_empty(),
        "Supply objects to upsert or deleteIds"
    );
    ensure!(
        upserted.len() + p.delete_ids.len() <= 1000,
        "At most 1000 canvas objects per edit"
    );
    canvas::validate(&p.nodes, &p.edges)?;
    media::validate(&p.media)?;
    let mut unique = HashSet::new();
    for id in upserted
        .iter()
        .copied()
        .chain(p.delete_ids.iter().map(String::as_str))
    {
        ensure!(identifier(id), "Invalid canvas ID: {id}");
        ensure!(
            unique.insert(id),
            "Duplicate or both upserted/deleted ID: {id}"
        );
    }
    ensure!(
        p.delete_ids.is_empty() || p.confirm_delete,
        "Deleting canvas objects requires confirmDelete=true"
    );
    let mut point_count = 0usize;
    for s in &p.strokes {
        ensure!(
            !s.points.is_empty() && s.points.len() <= 5000,
            "A stroke requires 1..5000 points"
        );
        ensure!(
            s.line_width.is_finite() && (0.5..=64.0).contains(&s.line_width),
            "Pen lineWidth must be in 0.5..64"
        );
        if let Some(color) = &s.color {
            color.validate()?;
        }
        for point in &s.points {
            ensure!(
                matches!(point.len(), 2 | 3),
                "Stroke points must be [x,y] or [x,y,pressure]"
            );
            validate_position([point[0], point[1]])?;
            if point.len() == 3 {
                ensure!(
                    point[2].is_finite() && (0.0..=1.0).contains(&point[2]),
                    "Pressure must be in 0..1"
                );
            }
        }
        point_count += s.points.len();
    }
    ensure!(point_count <= 50_000, "At most 50000 pen points per call");
    for c in &p.curves {
        validate_position(c.source)?;
        validate_position(c.target)?;
        ensure!(c.source != c.target, "Curve endpoints must differ");
        if let Some(width) = c.stroke_width {
            ensure!(
                width.is_finite() && (0.5..=64.0).contains(&width),
                "Curve strokeWidth must be in 0.5..64"
            );
        }
        if let Some(color) = &c.label_color {
            color.validate()?;
        }
        let e = curve_edge(c);
        canvas::validate(&[], &[e])?;
    }
    for f in &p.frames {
        validate_position([f.bounds[0], f.bounds[1]])?;
        validate_size([f.bounds[2], f.bounds[3]])?;
        ensure!(
            f.title.len() <= 2000,
            "Frame title must be at most 2000 UTF-8 bytes"
        );
        if let Some(color) = &f.background {
            color.validate()?;
        }
        if let Some(children) = &f.child_ids {
            ensure!(children.len() <= 1000, "At most 1000 children per frame");
            let mut unique = HashSet::new();
            for id in children {
                ensure!(identifier(id), "Invalid frame member ID: {id}");
                ensure!(
                    unique.insert(id) && id != &f.id && !p.delete_ids.contains(id),
                    "Duplicate, deleted or self-referencing frame member: {id}"
                );
            }
        }
    }
    for link in &p.links {
        validate_position(link.position)?;
        if let Some(size) = link.size {
            validate_size(size)?;
        }
        ensure!(
            link.title.len() <= 2000 && link.description.len() <= 4000,
            "Link title/description is too long"
        );
        match &link.target {
            LinkTarget::External { url } => {
                ensure!(
                    url.len() <= 4096
                        && !url.chars().any(char::is_whitespace)
                        && (url.starts_with("https://") || url.starts_with("http://")),
                    "External links must be explicit http:// or https:// URLs without whitespace"
                );
                let parsed = url::Url::parse(url).context("Invalid external link URL")?;
                ensure!(
                    parsed.has_host()
                        && parsed.username().is_empty()
                        && parsed.password().is_none(),
                    "External links require a host and must not contain credentials"
                );
            }
            LinkTarget::Document { document_id } => {
                ensure!(identifier(document_id), "Invalid link documentId")
            }
        }
    }
    Ok(())
}

pub(crate) fn stroke_props(s: &Stroke) -> Value {
    let min_x = s.points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let min_y = s.points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let max_x = s
        .points
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = s
        .points
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max);
    // Matches brush-tool.ts: pad the tight bounding box by lineWidth/2 on each side,
    // then express points relative to that padded box without losing pressure.
    let x = min_x - s.line_width / 2.0;
    let y = min_y - s.line_width / 2.0;
    let points: Vec<Vec<f64>> = s
        .points
        .iter()
        .map(|p| {
            let mut p = p.clone();
            p[0] -= x;
            p[1] -= y;
            p
        })
        .collect();
    json!({"type":"brush","xywh":serde_json::to_string(&[x,y,max_x-min_x+s.line_width,max_y-min_y+s.line_width]).unwrap(),
        "points":points,"lineWidth":s.line_width,"color":canvas::themed(&s.color,"#1e293b","#e2e8f0"),"rotate":0})
}
fn curve_edge(c: &Curve) -> Edge {
    Edge {
        id: c.id.clone(),
        source: "curve-source".into(),
        target: "curve-target".into(),
        text: c.label.clone(),
        routing: Routing::Curve,
        source_anchor: None,
        target_anchor: None,
        start_endpoint: if c.start_arrow {
            canvas::Endpoint::Arrow
        } else {
            canvas::Endpoint::None
        },
        end_endpoint: Some(if c.end_arrow {
            canvas::Endpoint::Arrow
        } else {
            canvas::Endpoint::None
        }),
        stroke_color: c.stroke.clone(),
    }
}
fn curve_props(c: &Curve) -> Value {
    let mut props = canvas::edge_props(
        &curve_edge(c),
        [c.source[0], c.source[1], 0.0, 0.0],
        [c.target[0], c.target[1], 0.0, 0.0],
    );
    props["source"] = json!({"position":c.source});
    props["target"] = json!({"position":c.target});
    props["strokeWidth"] = json!(c.stroke_width.unwrap_or(2.0));
    if c.label_color.is_some() {
        props["labelStyle"]["color"] = canvas::themed(&c.label_color, "#172554", "#e2e8f0");
    }
    props
}

pub(crate) fn supported_block(v: &Value) -> bool {
    matches!(
        v["sys:flavour"].as_str(),
        Some(
            "affine:frame"
                | "affine:bookmark"
                | "affine:embed-linked-doc"
                | "affine:image"
                | "affine:attachment"
        )
    )
}
fn frame_children(v: &Value) -> Vec<String> {
    let mut ids = HashSet::new();
    if let Some(o) = v["prop:childElementIds"].as_object() {
        ids.extend(
            o.iter()
                .filter(|(_, v)| v.as_bool() == Some(true))
                .map(|(k, _)| k.clone()),
        );
    }
    if let Some(o) = v.as_object() {
        ids.extend(
            o.iter()
                .filter(|(_, v)| v.as_bool() == Some(true))
                .filter_map(|(k, _)| k.strip_prefix("prop:childElementIds.").map(str::to_owned)),
        );
    }
    let mut ids: Vec<_> = ids.into_iter().collect();
    ids.sort();
    ids
}
struct State {
    elements: JsonMap<String, Value>,
    blocks: JsonMap<String, Value>,
    surface: String,
    surface_children: HashSet<String>,
}
impl State {
    fn load(doc: &Doc) -> Result<Self> {
        let elements = serde_json::to_value(canvas::elements(doc)?.to_json(&doc.transact()))?;
        let blocks = documents::root_json(doc, "blocks")?;
        let blocks = blocks
            .as_object()
            .context("Invalid document blocks")?
            .clone();
        let (surface, value) = blocks
            .iter()
            .find(|(_, b)| b["sys:flavour"] == "affine:surface")
            .context("Missing surface")?;
        let surface_children = value["sys:children"]
            .as_array()
            .context("Invalid surface children")?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        Ok(Self {
            elements: elements
                .as_object()
                .context("Invalid surface elements")?
                .clone(),
            surface: surface.clone(),
            blocks,
            surface_children,
        })
    }
    fn get(&self, id: &str) -> Option<&Value> {
        self.elements.get(id).or_else(|| self.blocks.get(id))
    }
    fn editable(&self, id: &str) -> Result<()> {
        ensure!(
            !self.elements.values().any(|v| v["type"] == "group"),
            "Editing grouped canvases is not supported; ungroup in AFFiNE first"
        );
        let mut queue = vec![id.to_owned()];
        let mut seen = HashSet::new();
        while let Some(id) = queue.pop() {
            ensure!(seen.insert(id.clone()), "Cyclic frame membership");
            if let Some(v) = self.get(&id) {
                ensure!(
                    v["lockedBySelf"] != true && v["prop:lockedBySelf"] != true,
                    "Canvas element or ancestor {id} is locked; unlock in AFFiNE first"
                );
            }
            queue.extend(
                self.blocks
                    .iter()
                    .filter(|(_, v)| {
                        v["sys:flavour"] == "affine:frame" && frame_children(v).contains(&id)
                    })
                    .map(|(k, _)| k.clone()),
            );
        }
        Ok(())
    }
    fn supported(&self, id: &str) -> bool {
        self.elements
            .get(id)
            .is_some_and(|v| matches!(v["type"].as_str(), Some("shape" | "connector" | "brush")))
            || (self.surface_children.contains(id)
                && self.blocks.get(id).is_some_and(supported_block))
    }
    fn validate_frames(&self) -> Result<()> {
        let mut parents = HashMap::new();
        for (id, f) in self
            .blocks
            .iter()
            .filter(|(_, v)| v["sys:flavour"] == "affine:frame")
        {
            for child in frame_children(f) {
                ensure!(
                    child != *id
                        && self.get(&child).is_some_and(|v| matches!(
                            v["type"].as_str(),
                            Some("shape" | "connector" | "brush")
                        ) || v["prop:xywh"].is_string()),
                    "Frame {id} references missing/non-canvas child {child}"
                );
                ensure!(
                    parents.insert(child.clone(), id.clone()).is_none(),
                    "Canvas object {child} belongs to multiple frames"
                );
            }
        }
        for child in parents.keys() {
            let mut seen = HashSet::new();
            let mut next = Some(child);
            while let Some(id) = next {
                ensure!(seen.insert(id), "Cyclic frame membership");
                next = parents.get(id);
            }
        }
        Ok(())
    }
}

pub(crate) fn canvas_blocks(doc: &Doc) -> Result<Vec<Value>> {
    let state = State::load(doc)?;
    let mut result: Vec<_> = state
        .blocks
        .iter()
        .filter(|(id, v)| state.surface_children.contains(*id) && supported_block(v))
        .map(|(id, v)| json!({"id":id,"properties":v}))
        .collect();
    result.sort_by(|a, b| {
        a["properties"]["prop:index"]
            .as_str()
            .cmp(&b["properties"]["prop:index"].as_str())
            .then(a["id"].as_str().cmp(&b["id"].as_str()))
    });
    Ok(result)
}

pub(crate) fn apply(doc: &Doc, p: &EditCanvas, prepared: &[media::Prepared]) -> Result<Value> {
    validate(p)?;
    let before = State::load(doc)?;
    for id in ids(p) {
        before.editable(id)?;
    }
    for frame in &p.frames {
        if let Some(children) = &frame.child_ids {
            for child in children {
                before.editable(child)?;
            }
        }
    }
    let elements = canvas::elements(doc)?;
    let mut geometry: HashMap<_, _> = before
        .elements
        .iter()
        .filter(|(id, v)| v["type"] == "shape" && !p.delete_ids.contains(id))
        .map(|(id, v)| Ok((id.clone(), canvas::bounds(v)?)))
        .collect::<Result<_>>()?;
    for node in &p.nodes {
        let old = geometry.get(&node.id).copied();
        let [x, y] = node
            .position
            .or_else(|| old.map(|b| [b[0], b[1]]))
            .context(
                "New canvas nodes require a position; use affine_create_diagram for auto-layout",
            )?;
        let [w, h] = node
            .size
            .or_else(|| old.map(|b| [b[2], b[3]]))
            .unwrap_or([260.0, 120.0]);
        geometry.insert(node.id.clone(), [x, y, w, h]);
    }
    for n in &p.nodes {
        ensure!(
            !before.blocks.contains_key(&n.id),
            "Element ID {} collides with a document block",
            n.id
        );
        canvas::upsert(
            doc,
            &elements,
            &n.id,
            canvas::shape_props(n, geometry[&n.id]),
        )?;
    }
    for stroke in &p.strokes {
        ensure!(
            !before.blocks.contains_key(&stroke.id),
            "Element ID {} collides with a document block",
            stroke.id
        );
        canvas::upsert(doc, &elements, &stroke.id, stroke_props(stroke))?;
    }
    for curve in &p.curves {
        ensure!(
            !before.blocks.contains_key(&curve.id),
            "Element ID {} collides with a document block",
            curve.id
        );
        canvas::upsert(doc, &elements, &curve.id, curve_props(curve))?;
    }

    for edge in &p.edges {
        ensure!(
            !before.blocks.contains_key(&edge.id),
            "Element ID {} collides with a document block",
            edge.id
        );
        ensure!(
            geometry.contains_key(&edge.source) && geometry.contains_key(&edge.target),
            "Connector {} endpoints must reference existing, non-deleted shapes",
            edge.id
        );
        canvas::upsert(
            doc,
            &elements,
            &edge.id,
            canvas::edge_props(edge, geometry[&edge.source], geometry[&edge.target]),
        )?;
    }
    for link in &p.links {
        let [x, y] = link.position;
        let [w, h] = link.size.unwrap_or([460.0, 180.0]);
        let mut props = json!({"xywh":serde_json::to_string(&[x,y,w,h])?,"rotate":0,"style":"horizontal","title":link.title,"description":link.description,"caption":null});
        let flavour = match &link.target {
            LinkTarget::External { url } => {
                props["url"] = json!(url);
                "affine:bookmark"
            }
            LinkTarget::Document { document_id } => {
                props["pageId"] = json!(document_id);
                "affine:embed-linked-doc"
            }
        };
        upsert_block(doc, &link.id, flavour, &props, None)?;
    }
    for media in prepared {
        upsert_block(doc, &media.id, media.flavour, &media.props, None)?;
    }
    for frame in &p.frames {
        let mut props = json!({"xywh":serde_json::to_string(&frame.bounds)?});
        if let Some(color) = &frame.background {
            props["background"] = serde_json::to_value(color)?;
        } else if !before.blocks.contains_key(&frame.id) {
            props["background"] = json!("transparent");
        }
        if let Some(children) = &frame.child_ids {
            props["childElementIds"] = json!(
                children
                    .iter()
                    .map(|id| (id, true))
                    .collect::<HashMap<_, _>>()
            );
        } else if !before.blocks.contains_key(&frame.id) {
            props["childElementIds"] = json!({});
        }
        if let Some(index) = frame.presentation_index {
            props["presentationIndex"] = json!(index);
        }
        upsert_block(doc, &frame.id, "affine:frame", &props, Some(&frame.title))?;
    }
    State::load(doc)?.validate_frames()?;
    let deleted = delete(doc, &p.delete_ids)?;
    Ok(
        json!({"upsertedIds":ids(p),"deletedIds":deleted,"importedMedia":prepared.iter().map(media::Prepared::result).collect::<Vec<_>>()}),
    )
}

/// BlockSuite flat storage: nested block properties are dotted prop:* keys,
/// while rich text remains a real Y.Text. Never replace the whole block map.
fn upsert_block(
    doc: &Doc,
    id: &str,
    flavour: &str,
    props: &Value,
    title: Option<&str>,
) -> Result<()> {
    let state = State::load(doc)?;
    ensure!(
        !state.elements.contains_key(id),
        "Block ID {id} collides with a surface element"
    );
    state.editable(id)?;
    let elements = canvas::elements(doc)?;
    let mut txn = doc.transact_mut();
    let blocks = txn.get_map("blocks").context("Missing block map")?;
    let block = if let Some(existing) = blocks.get(&txn, id) {
        let Out::YMap(block) = existing else {
            bail!("Invalid canvas block {id}");
        };
        ensure!(
            block
                .get(&txn, "sys:flavour")
                .is_some_and(|v| v.to_string(&txn) == flavour),
            "Existing block {id} has a different flavour"
        );
        ensure!(
            state.surface_children.contains(id),
            "Existing block {id} is not a direct surface child"
        );
        ensure!(
            block
                .get(&txn, "sys:version")
                .is_some_and(|v| v.to_string(&txn) == "1"),
            "Unsupported canvas block version for {id}"
        );
        block
    } else {
        let index = canvas::allocate_index(&txn, &elements)?;
        let block = blocks.insert(&mut txn, id, MapPrelim::default());
        block.insert(&mut txn, "sys:id", id);
        block.insert(&mut txn, "sys:flavour", flavour);
        block.insert(&mut txn, "sys:version", 1i64);
        block.insert(&mut txn, "sys:children", ArrayPrelim::default());
        block.insert(&mut txn, "prop:index", index);
        block.insert(&mut txn, "prop:lockedBySelf", false);
        let Out::YMap(surface) = blocks
            .get(&txn, state.surface.as_str())
            .context("Missing surface block")?
        else {
            bail!("Invalid surface block");
        };
        let Out::YArray(children) = surface
            .get(&txn, "sys:children")
            .context("Missing surface children")?
        else {
            bail!("Invalid surface children");
        };
        children.push_back(&mut txn, id);
        block
    };
    for (key, value) in props
        .as_object()
        .context("Invalid canvas block properties")?
    {
        let root = format!("prop:{key}");
        let prefix = format!("{root}.");
        let old: Vec<_> = block
            .keys(&txn)
            .filter(|key| *key == root || key.starts_with(&prefix))
            .map(str::to_owned)
            .collect();
        for key in old {
            block.remove(&mut txn, &key);
        }
        let mut flat = Vec::new();
        flatten(&root, value, &mut flat);
        for (key, value) in flat {
            block.insert(
                &mut txn,
                key.as_str(),
                yrs::Any::from_json(&serde_json::to_string(value)?)?,
            );
        }
    }
    if let Some(title) = title {
        if let Some(Out::YText(text)) = block.get(&txn, "prop:title") {
            let length = text.len(&txn);
            text.remove_range(&mut txn, 0, length);
            text.insert(&mut txn, 0, title);
        } else {
            block.insert(&mut txn, "prop:title", TextPrelim::new(title));
        }
    }
    Ok(())
}
fn flatten<'a>(prefix: &str, value: &'a Value, out: &mut Vec<(String, &'a Value)>) {
    if let Some(object) = value.as_object().filter(|o| !o.is_empty()) {
        for (key, value) in object {
            flatten(&format!("{prefix}.{key}"), value, out);
        }
    } else {
        out.push((prefix.to_owned(), value));
    }
}

fn delete(doc: &Doc, requested: &[String]) -> Result<Vec<String>> {
    let state = State::load(doc)?;
    let mut deleted: HashSet<_> = requested.iter().cloned().collect();
    for id in requested {
        if state.get(id).is_none() {
            continue;
        }
        ensure!(
            state.supported(id),
            "Canvas element {id} is missing or unsupported; note/page/surface deletion is forbidden"
        );
        state.editable(id)?;
        if let Some(block) = state.blocks.get(id) {
            ensure!(
                block["sys:version"] == 1,
                "Unsupported canvas block version for {id}"
            );
            ensure!(
                block["sys:children"].as_array().is_some_and(Vec::is_empty),
                "Cannot delete a canvas block with nested document blocks"
            );
        }
    }
    for (id, value) in &state.elements {
        if value["type"] == "connector"
            && ["source", "target"].iter().any(|side| {
                value[*side]["id"]
                    .as_str()
                    .is_some_and(|id| deleted.contains(id))
            })
        {
            state.editable(id)?;
            deleted.insert(id.clone());
        }
    }
    let map = canvas::elements(doc)?;
    let mut txn = doc.transact_mut();
    let blocks = txn.get_map("blocks").context("Missing blocks")?;
    for id in &deleted {
        map.remove(&mut txn, id);
        blocks.remove(&mut txn, id);
    }
    let Out::YMap(surface) = blocks
        .get(&txn, state.surface.as_str())
        .context("Missing surface")?
    else {
        bail!("Invalid surface");
    };
    let Out::YArray(children) = surface
        .get(&txn, "sys:children")
        .context("Missing surface children")?
    else {
        bail!("Invalid surface children");
    };
    let positions: Vec<_> = children
        .iter(&txn)
        .enumerate()
        .filter(|(_, id)| deleted.contains(&id.clone().to_string(&txn)))
        .map(|(i, _)| i as u32)
        .collect();
    for index in positions.into_iter().rev() {
        children.remove_range(&mut txn, index, 1);
    }
    for (id, value) in &state.blocks {
        if value["sys:flavour"] != "affine:frame" || deleted.contains(id) {
            continue;
        }
        let remaining: Vec<_> = frame_children(value)
            .into_iter()
            .filter(|id| !deleted.contains(id))
            .collect();
        if remaining.len() == frame_children(value).len() {
            continue;
        }
        let Some(Out::YMap(frame)) = blocks.get(&txn, id) else {
            bail!("Invalid frame {id}");
        };
        let old: Vec<_> = frame
            .keys(&txn)
            .filter(|key| {
                *key == "prop:childElementIds" || key.starts_with("prop:childElementIds.")
            })
            .map(str::to_owned)
            .collect();
        for key in old {
            frame.remove(&mut txn, &key);
        }
        for id in remaining {
            frame.insert(
                &mut txn,
                format!("prop:childElementIds.{id}").as_str(),
                true,
            );
        }
    }
    let mut deleted: Vec<_> = deleted.into_iter().collect();
    deleted.sort();
    Ok(deleted)
}

pub(crate) fn erase(doc: &Doc, p: &EraseCanvas) -> Result<Value> {
    ensure!(p.confirm, "Erasing requires confirm=true");
    ensure!(
        !p.stroke_ids.is_empty() || !p.path.is_empty(),
        "Supply strokeIds or an eraser path"
    );
    ensure!(
        p.stroke_ids.len() <= 1000 && p.path.len() <= 1000,
        "At most 1000 stroke IDs/path points"
    );
    ensure!(
        p.radius.is_finite() && (0.0..=1000.0).contains(&p.radius),
        "Eraser radius must be in 0..1000"
    );
    for point in &p.path {
        validate_position(*point)?;
    }
    let state = State::load(doc)?;
    let mut ids = HashSet::new();
    ensure!(
        !state.elements.values().any(|v| v["type"] == "group"),
        "Erasing grouped canvases is not supported; ungroup in AFFiNE first"
    );
    state.validate_frames()?;
    for id in &p.stroke_ids {
        ensure!(identifier(id), "Invalid brush ID: {id}");
        if state.get(id).is_none() {
            continue;
        }
        ensure!(
            state.elements.get(id).is_some_and(|v| v["type"] == "brush"),
            "Eraser stroke ID {id} does not reference a brush"
        );
        state.editable(id)?;
        ids.insert(id.clone());
    }
    let mut skipped = Vec::new();
    let mut work = 0usize;
    if !p.path.is_empty() {
        for (id, value) in state.elements.iter().filter(|(_, v)| v["type"] == "brush") {
            let (points, width) = brush_path(value)?;
            work = work.saturating_add(points.len().saturating_mul(p.path.len()));
            ensure!(
                work <= 2_000_000,
                "Eraser sweep is too complex; use strokeIds or a shorter path"
            );
            if paths_touch(&points, &p.path, p.radius + width / 2.0) {
                if state.editable(id).is_err() {
                    skipped.push(id.clone());
                } else {
                    ids.insert(id.clone());
                }
            }
        }
    }
    skipped.sort();
    let deleted = delete(doc, &ids.into_iter().collect::<Vec<_>>())?;
    Ok(json!({"deletedIds":deleted,"skippedLockedIds":skipped,"wholeStrokeErasure":true}))
}

fn brush_path(v: &Value) -> Result<(Vec<[f64; 2]>, f64)> {
    let bound: [f64; 4] =
        serde_json::from_str(v["xywh"].as_str().context("Invalid brush bounds")?)?;
    ensure!(
        bound
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 2_000_001_000.0)
            && bound[2] > 0.0
            && bound[3] > 0.0,
        "Invalid brush bounds"
    );
    let width = v["lineWidth"].as_f64().context("Invalid brush lineWidth")?;
    ensure!(
        width.is_finite() && width > 0.0 && width <= 1000.0,
        "Invalid brush lineWidth"
    );
    let rotation = v["rotate"].as_f64().unwrap_or(0.0);
    ensure!(rotation.is_finite(), "Invalid brush rotation");
    let (sin, cos) = rotation.to_radians().sin_cos();
    let center = [bound[0] + bound[2] / 2.0, bound[1] + bound[3] / 2.0];
    let stored = v["points"].as_array().context("Invalid brush points")?;
    ensure!(
        !stored.is_empty() && stored.len() <= 100_000,
        "Invalid brush point count"
    );
    let mut points = Vec::new();
    for p in stored {
        let x = p[0].as_f64().context("Invalid brush x")? + bound[0] - center[0];
        let y = p[1].as_f64().context("Invalid brush y")? + bound[1] - center[1];
        let point = [center[0] + x * cos - y * sin, center[1] + x * sin + y * cos];
        ensure!(
            point
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 10_000_000_000.0),
            "Invalid brush coordinates"
        );
        points.push(point);
    }
    Ok((points, width))
}
fn segments(points: &[[f64; 2]]) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
    (0..points.len().saturating_sub(1).max(1))
        .map(|i| (points[i], points[(i + 1).min(points.len() - 1)]))
}
fn paths_touch(a: &[[f64; 2]], b: &[[f64; 2]], radius: f64) -> bool {
    segments(a).any(|(a0, a1)| {
        segments(b).any(|(b0, b1)| segment_distance_squared(a0, a1, b0, b1) <= radius * radius)
    })
}
fn point_distance_squared(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0] * d[0] + d[1] * d[1];
    let t = if length == 0.0 {
        0.0
    } else {
        ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length
    }
    .clamp(0.0, 1.0);
    (p[0] - a[0] - t * d[0]).powi(2) + (p[1] - a[1] - t * d[1]).powi(2)
}
fn segment_distance_squared(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let cd = [d[0] - c[0], d[1] - c[1]];
    let cross = |u: [f64; 2], v: [f64; 2]| u[0] * v[1] - u[1] * v[0];
    let denom = cross(ab, cd);
    if denom.abs() > f64::EPSILON {
        let ca = [c[0] - a[0], c[1] - a[1]];
        let t = cross(ca, cd) / denom;
        let u = cross(ca, ab) / denom;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            return 0.0;
        }
    }
    point_distance_squared(a, c, d)
        .min(point_distance_squared(b, c, d))
        .min(point_distance_squared(c, a, b))
        .min(point_distance_squared(d, a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_frame_locks_and_grouped_eraser_boundaries_are_respected() {
        use yrs::{Update, updates::decoder::Decode};
        let doc = Doc::new();
        doc.transact_mut()
            .apply_update(
                Update::decode_v1(include_bytes!("../tests/fixtures/native-drawing.bin")).unwrap(),
            )
            .unwrap();
        let edit: EditCanvas = serde_json::from_value(json!({"workspaceId":"w","documentId":"d",
            "frames":[{"id":"outer","title":"Outer frame","bounds":[0,0,1500,1500],"childIds":["js-frame"]}]})).unwrap();
        apply(&doc, &edit, &[]).unwrap();
        State::load(&doc).unwrap().validate_frames().unwrap();
        {
            let mut txn = doc.transact_mut();
            let Some(Out::YMap(frame)) = txn.get_map("blocks").unwrap().get(&txn, "outer") else {
                panic!()
            };
            frame.insert(&mut txn, "prop:lockedBySelf", true);
        }
        assert!(State::load(&doc).unwrap().editable("js-pen").is_err());
        let p: EraseCanvas = serde_json::from_value(
            json!({"workspaceId":"w","documentId":"d","path":[[50,300],[500,300]],"confirm":true}),
        )
        .unwrap();
        let result = erase(&doc, &p).unwrap();
        assert_eq!(result["deletedIds"], json!([]));
        assert_eq!(result["skippedLockedIds"], json!(["js-pen"]));
        let elements = canvas::elements(&doc).unwrap();
        canvas::upsert(
            &doc,
            &elements,
            "unsupported-group",
            json!({"type":"group"}),
        )
        .unwrap();
        assert!(
            erase(&doc, &p)
                .unwrap_err()
                .to_string()
                .contains("grouped canvases")
        );
    }
    #[test]
    fn reads_and_edits_independent_native_brush_curve_frame_links_and_media() {
        use yrs::{GetString, Update, updates::decoder::Decode};
        let doc = Doc::new();
        doc.transact_mut()
            .apply_update(
                Update::decode_v1(include_bytes!("../tests/fixtures/native-drawing.bin")).unwrap(),
            )
            .unwrap();
        let state = State::load(&doc).unwrap();
        state.validate_frames().unwrap();
        assert_eq!(state.elements["js-curve"]["mode"], 2);
        assert_eq!(
            state.elements["js-curve"]["source"],
            json!({"position":[500,100]})
        );
        assert_eq!(canvas_blocks(&doc).unwrap().len(), 5);
        assert_eq!(
            state.blocks["js-image"]["prop:sourceId"],
            "WSDpuCmfHg_JdoyVwWVLD_uX1o7jfM1xmB9kgtLxmjs"
        );
        assert_eq!(state.blocks["js-link"]["prop:url"], "https://affine.pro/");
        assert_eq!(state.blocks["js-doc-link"]["prop:pageId"], "fixture-page");
        let original_image = state.blocks["js-image"].clone();
        let original_title = {
            let txn = doc.transact();
            let Some(Out::YMap(frame)) = txn.get_map("blocks").unwrap().get(&txn, "js-frame")
            else {
                panic!()
            };
            let Some(Out::YText(title)) = frame.get(&txn, "prop:title") else {
                panic!()
            };
            title
        };
        let edit: EditCanvas = serde_json::from_value(json!({"workspaceId":"w","documentId":"js-drawing",
            "frames":[{"id":"js-frame","title":"Rust edited independent frame 🦀","bounds":[50,50,800,500]}]})).unwrap();
        apply(&doc, &edit, &[]).unwrap();
        let txn = doc.transact();
        let Some(Out::YMap(frame)) = txn.get_map("blocks").unwrap().get(&txn, "js-frame") else {
            panic!()
        };
        let Some(Out::YText(title)) = frame.get(&txn, "prop:title") else {
            panic!()
        };
        assert_eq!(original_title, title);
        assert_eq!(title.get_string(&txn), "Rust edited independent frame 🦀");
        drop(txn);
        let sweep: EraseCanvas = serde_json::from_value(json!({"workspaceId":"w","documentId":"js-drawing","path":[[50,300],[500,300]],"radius":0,"confirm":true})).unwrap();
        assert_eq!(
            erase(&doc, &sweep).unwrap()["deletedIds"],
            json!(["js-pen"])
        );
        let state = State::load(&doc).unwrap();
        assert_eq!(
            frame_children(&state.blocks["js-frame"]),
            vec!["js-curve", "js-image"]
        );
        assert!(state.elements.contains_key("js-curve"));
        assert_eq!(state.blocks["js-image"], original_image);
    }
    #[test]
    fn pressure_and_brush_geometry_match_native_relative_coordinates() {
        let p: EditCanvas = serde_json::from_value(json!({"workspaceId":"w","documentId":"d","strokes":[{"id":"pen","points":[[100,200,0.2],[300,400,0.9]],"lineWidth":8}]})).unwrap();
        validate(&p).unwrap();
        let value = stroke_props(&p.strokes[0]);
        assert_eq!(value["xywh"], "[96.0,196.0,208.0,208.0]");
        assert_eq!(
            value["points"],
            json!([[4.0, 4.0, 0.2], [204.0, 204.0, 0.9]])
        );
        let (points, width) = brush_path(&value).unwrap();
        assert_eq!(points, vec![[100.0, 200.0], [300.0, 400.0]]);
        assert_eq!(width, 8.0);
    }
    #[test]
    fn eraser_handles_crossings_parallel_paths_dots_and_rotation() {
        assert!(paths_touch(
            &[[0.0, 0.0], [10.0, 10.0]],
            &[[0.0, 10.0], [10.0, 0.0]],
            0.0
        ));
        assert!(!paths_touch(
            &[[0.0, 0.0], [10.0, 0.0]],
            &[[0.0, 5.0], [10.0, 5.0]],
            4.0
        ));
        assert!(paths_touch(&[[0.0, 0.0]], &[[3.0, 4.0]], 5.0));
        let v = json!({"xywh":"[-2,-2,14,4]","points":[[2,2],[12,2]],"lineWidth":4,"rotate":90});
        let (points, _) = brush_path(&v).unwrap();
        assert!(paths_touch(&points, &[[0.0, -4.0], [10.0, -4.0]], 0.01));
        assert!(!paths_touch(&points, &[[0.0, 4.0]], 0.01));
    }
    #[test]
    fn invalid_pressure_and_dangerous_links_are_rejected() {
        for body in [
            json!({"strokes":[{"id":"p","points":[[0,0,2]]}]}),
            json!({"links":[{"id":"l","position":[0,0],"target":{"kind":"external","url":"javascript:alert(1)"}}]}),
            json!({"links":[{"id":"l","position":[0,0],"target":{"kind":"external","url":"https://user:password@example.com"}}]}),
        ] {
            let mut body = body;
            body["workspaceId"] = json!("w");
            body["documentId"] = json!("d");
            let p: EditCanvas = serde_json::from_value(body).unwrap();
            assert!(validate(&p).is_err());
        }
    }
}
