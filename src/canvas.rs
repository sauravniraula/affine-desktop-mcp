//! Native AFFiNE v0.27.4 surface elements, not text notes or raster diagrams.
use anyhow::{Context, Result, bail, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use yrs::{Doc, In, Map, MapPrelim, MapRef, Out, ReadTxn, Text, TextPrelim, Transact};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Shape {
    Rect,
    #[default]
    RoundedRect,
    Ellipse,
    Diamond,
    Triangle,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    #[default]
    General,
    Scribbled,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Routing {
    Straight,
    #[default]
    Orthogonal,
    Curve,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    Top,
    Right,
    Bottom,
    Left,
}
impl Anchor {
    fn position(self) -> [f64; 2] {
        match self {
            Self::Top => [0.5, 0.0],
            Self::Right => [1.0, 0.5],
            Self::Bottom => [0.5, 1.0],
            Self::Left => [0.0, 0.5],
        }
    }
    fn opposite(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Right => Self::Left,
            Self::Bottom => Self::Top,
            Self::Left => Self::Right,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Endpoint {
    #[default]
    None,
    Arrow,
    Circle,
    Diamond,
    Triangle,
}
impl Endpoint {
    fn native(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Arrow => "Arrow",
            Self::Circle => "Circle",
            Self::Diamond => "Diamond",
            Self::Triangle => "Triangle",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Color {
    /// CSS hex color (#RGB, #RGBA, #RRGGBB or #RRGGBBAA).
    Solid(String),
    Theme {
        light: String,
        dark: String,
    },
}
impl Color {
    pub(crate) fn validate(&self) -> Result<()> {
        let valid = |s: &str| {
            matches!(s.len(), 4 | 5 | 7 | 9)
                && s.starts_with('#')
                && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
        };
        ensure!(
            match self {
                Self::Solid(color) => valid(color),
                Self::Theme { light, dark } => valid(light) && valid(dark),
            },
            "Colors must be CSS hex strings or {{light, dark}} hex colors"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Node {
    /// Stable surface-element ID; unique across nodes and edges.
    pub id: String,
    /// Editable rich-text label; use newlines for multi-line labels (max 2000 bytes).
    pub text: String,
    #[serde(default)]
    pub shape: Shape,
    /// Optional [x, y] canvas position. Creation auto-layouts unspecified positions.
    /// Editing requires a position for new nodes; existing nodes retain their position.
    pub position: Option<[f64; 2]>,
    /// Optional [width, height]; defaults to [260, 120], or existing size when editing.
    pub size: Option<[f64; 2]>,
    #[serde(default)]
    pub style: Style,
    pub fill_color: Option<Color>,
    pub stroke_color: Option<Color>,
    pub text_color: Option<Color>,
    /// Label font size (8..72), defaults to 20.
    pub font_size: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Edge {
    pub id: String,
    /// Source and target must be native shape IDs, not note text or connector IDs.
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub routing: Routing,
    /// Optional edge attachment sides. Defaults face the other shape.
    pub source_anchor: Option<Anchor>,
    pub target_anchor: Option<Anchor>,
    #[serde(default)]
    pub start_endpoint: Endpoint,
    /// Defaults to an arrow at the target.
    pub end_endpoint: Option<Endpoint>,
    pub stroke_color: Option<Color>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Direction {
    #[default]
    Tb,
    Lr,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDiagram {
    pub workspace_id: String,
    pub title: String,
    /// Use a stable documentId for safe retries; conflicts never overwrite an existing doc.
    pub document_id: Option<String>,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    /// TB (top-to-bottom, default) or LR (left-to-right) layered DAG layout.
    /// Cyclic diagrams must explicitly position every node.
    #[serde(default)]
    pub direction: Direction,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditCanvas {
    pub workspace_id: String,
    pub document_id: String,
    /// Upsert native shapes by stable ID; appearance/label defaults are applied,
    /// but unspecified position/size retain existing geometry. Other elements are preserved.
    #[serde(default)]
    pub nodes: Vec<Node>,
    /// Upsert bound connectors by stable ID. Endpoints may reference existing shapes.
    #[serde(default)]
    pub edges: Vec<Edge>,
    /// Native pen strokes; absolute [x,y] or [x,y,pressure] points.
    #[serde(default)]
    pub strokes: Vec<crate::drawing::Stroke>,
    /// Free-floating native curved connectors with absolute source/target points.
    #[serde(default)]
    pub curves: Vec<crate::drawing::Curve>,
    /// Native frame blocks with optional explicit child element IDs.
    #[serde(default)]
    pub frames: Vec<crate::drawing::Frame>,
    /// Native external bookmark cards or same-workspace document link cards.
    #[serde(default)]
    pub links: Vec<crate::drawing::Link>,
    /// Import images/attachments from the data-directory's mcp-imports folder.
    #[serde(default)]
    pub media: Vec<crate::media::Media>,
    /// Delete supported canvas elements/blocks plus incident connectors.
    /// Removing a frame preserves its contents. Note/page/surface blocks are never deleted.
    #[serde(default)]
    pub delete_ids: Vec<String>,
    /// Must be true when deleteIds is nonempty.
    #[serde(default)]
    pub confirm_delete: bool,
    /// Explicitly repair missing internal IDs on existing shapes, brushes and
    /// connectors without replacing geometry. Conflicting IDs are never rewritten.
    /// Element/ancestor locks and grouped-canvas restrictions still apply.
    #[serde(default)]
    pub repair_missing_ids: bool,
}

/// Create the document and its vector artwork in one atomic offline write.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateCanvas {
    pub workspace_id: String,
    pub title: String,
    /// Stable ID for safe retries; a conflicting existing document is never overwritten.
    pub document_id: Option<String>,
    /// Optional shapes. Unpositioned shapes use TB auto-layout.
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    /// Editable pen strokes using absolute canvas points.
    #[serde(default)]
    pub strokes: Vec<crate::drawing::Stroke>,
    #[serde(default)]
    pub curves: Vec<crate::drawing::Curve>,
    #[serde(default)]
    pub frames: Vec<crate::drawing::Frame>,
}

pub fn validate(nodes: &[Node], edges: &[Edge]) -> Result<()> {
    ensure!(
        nodes.len() <= 200 && edges.len() <= 500,
        "At most 200 nodes and 500 edges per call"
    );
    let mut ids = HashSet::new();
    for (id, text) in nodes
        .iter()
        .map(|n| (&n.id, &n.text))
        .chain(edges.iter().map(|e| (&e.id, &e.text)))
    {
        ensure!(
            crate::storage::identifier(id),
            "Invalid surface element ID: {id}"
        );
        ensure!(ids.insert(id), "Duplicate node/edge ID: {id}");
        ensure!(
            text.len() <= 2000,
            "Element label is longer than 2000 UTF-8 bytes: {id}"
        );
    }
    ensure!(
        nodes
            .iter()
            .map(|n| n.text.len())
            .chain(edges.iter().map(|e| e.text.len()))
            .sum::<usize>()
            <= 100_000,
        "Combined labels must be at most 100000 UTF-8 bytes"
    );
    for node in nodes {
        for color in [&node.fill_color, &node.stroke_color, &node.text_color]
            .into_iter()
            .flatten()
        {
            color.validate()?;
        }
        if let Some(font_size) = node.font_size {
            ensure!(
                font_size.is_finite() && (8.0..=72.0).contains(&font_size),
                "fontSize must be 8..72"
            );
        }
        if let Some([x, y]) = node.position {
            ensure!(
                [x, y]
                    .into_iter()
                    .all(|v| v.is_finite() && v.abs() <= 1_000_000.0),
                "Invalid node position"
            );
        }
        if let Some([w, h]) = node.size {
            ensure!(
                [w, h]
                    .into_iter()
                    .all(|v| v.is_finite() && (20.0..=10_000.0).contains(&v)),
                "Node dimensions must be 20..10000"
            );
        }
    }
    for edge in edges {
        ensure!(
            crate::storage::identifier(&edge.source) && crate::storage::identifier(&edge.target),
            "Invalid connector endpoint ID"
        );
        if let Some(color) = &edge.stroke_color {
            color.validate()?;
        }
    }
    Ok(())
}

/// Allocate native fractional-indexing integer keys. Existing fractional suffixes
/// are skipped on append, just as generateKeyBetween(last, null) does in AFFiNE.
pub fn next_index(last: Option<&str>) -> Result<String> {
    const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let Some(last) = last else {
        return Ok("a0".into());
    };
    let bytes = last.as_bytes();
    let head = *bytes.first().context("Empty canvas layer index")?;
    let length = match head {
        b'a'..=b'z' => (head - b'a') as usize + 2,
        b'A'..=b'Z' => (b'Z' - head) as usize + 2,
        _ => bail!("Unsupported canvas layer index: {last}"),
    };
    ensure!(
        bytes.len() >= length && bytes[1..].iter().all(|b| DIGITS.contains(b)),
        "Invalid canvas layer index: {last}"
    );
    let mut digits = bytes[1..length].to_vec();
    for i in (0..digits.len()).rev() {
        let index = DIGITS.iter().position(|b| *b == digits[i]).unwrap();
        if index < DIGITS.len() - 1 {
            digits[i] = DIGITS[index + 1];
            return Ok(format!("{}{}", head as char, String::from_utf8(digits)?));
        }
        digits[i] = b'0';
    }
    if head == b'Z' {
        return Ok("a0".into());
    }
    ensure!(head != b'z', "Canvas layer index exhausted");
    if head >= b'a' {
        digits.push(b'0');
    } else {
        digits.pop();
    }
    Ok(format!(
        "{}{}",
        (head + 1) as char,
        String::from_utf8(digits)?
    ))
}

pub fn layout(nodes: &[Node], edges: &[Edge], direction: Direction) -> Result<Vec<[f64; 4]>> {
    validate(nodes, edges)?;
    ensure!(!nodes.is_empty(), "A diagram requires at least one node");
    let indices: HashMap<_, _> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let mut outgoing = vec![Vec::new(); nodes.len()];
    let mut indegree = vec![0usize; nodes.len()];
    for edge in edges {
        let source = *indices
            .get(edge.source.as_str())
            .with_context(|| format!("Unknown source shape: {}", edge.source))?;
        let target = *indices
            .get(edge.target.as_str())
            .with_context(|| format!("Unknown target shape: {}", edge.target))?;
        outgoing[source].push(target);
        indegree[target] += 1;
    }
    let mut ranks = vec![0usize; nodes.len()];
    let mut queue: VecDeque<_> = indegree
        .iter()
        .enumerate()
        .filter(|(_, degree)| **degree == 0)
        .map(|(i, _)| i)
        .collect();
    let mut visited = 0;
    while let Some(source) = queue.pop_front() {
        visited += 1;
        for &target in &outgoing[source] {
            ranks[target] = ranks[target].max(ranks[source] + 1);
            indegree[target] -= 1;
            if indegree[target] == 0 {
                queue.push_back(target);
            }
        }
    }
    ensure!(
        visited == nodes.len() || nodes.iter().all(|n| n.position.is_some()),
        "Cyclic diagrams require explicit positions for every node"
    );
    let mut layers = vec![Vec::new(); ranks.iter().max().copied().unwrap_or(0) + 1];
    for (i, &rank) in ranks.iter().enumerate() {
        layers[rank].push(i);
    }
    let width = nodes
        .iter()
        .map(|n| n.size.unwrap_or([260.0, 120.0])[0])
        .fold(260.0, f64::max);
    let height = nodes
        .iter()
        .map(|n| n.size.unwrap_or([260.0, 120.0])[1])
        .fold(120.0, f64::max);
    let widest = layers.iter().map(Vec::len).max().unwrap_or(1);
    let mut bounds = vec![[0.0; 4]; nodes.len()];
    for (rank, layer) in layers.iter().enumerate() {
        for (slot, &i) in layer.iter().enumerate() {
            let across = slot as f64 + (widest - layer.len()) as f64 / 2.0;
            let [x, y] = nodes[i].position.unwrap_or(match direction {
                Direction::Tb => [
                    100.0 + across * (width + 80.0),
                    100.0 + rank as f64 * (height + 100.0),
                ],
                Direction::Lr => [
                    100.0 + rank as f64 * (width + 120.0),
                    100.0 + across * (height + 80.0),
                ],
            });
            let [w, h] = nodes[i].size.unwrap_or([260.0, 120.0]);
            bounds[i] = [x, y, w, h];
        }
    }
    Ok(bounds)
}

pub fn elements(doc: &Doc) -> Result<MapRef> {
    let txn = doc.transact();
    let blocks = txn.get_map("blocks").context("Missing blocks")?;
    let mut surfaces = Vec::new();
    for (_, block) in blocks.iter(&txn) {
        if let Out::YMap(block) = block
            && block
                .get(&txn, "sys:flavour")
                .is_some_and(|v| v.to_string(&txn) == "affine:surface")
        {
            surfaces.push(block);
        }
    }
    ensure!(surfaces.len() == 1, "Expected exactly one AFFiNE surface");
    let Some(Out::YMap(wrapper)) = surfaces[0].get(&txn, "prop:elements") else {
        bail!("Unsupported native surface element container");
    };
    if wrapper
        .get(&txn, "type")
        .is_some_and(|v| v.to_string(&txn) == "$blocksuite:internal:native$")
    {
        let Some(Out::YMap(value)) = wrapper.get(&txn, "value") else {
            bail!("Native surface wrapper has no Y.Map value");
        };
        Ok(value)
    } else {
        ensure!(
            !wrapper.contains_key(&txn, "type"),
            "Unsupported surface wrapper"
        );
        Ok(wrapper)
    }
}

pub(crate) fn themed(color: &Option<Color>, light: &str, dark: &str) -> Value {
    color
        .as_ref()
        .map(|c| serde_json::to_value(c).unwrap())
        .unwrap_or_else(|| json!({"light":light,"dark":dark}))
}

pub fn shape_props(node: &Node, bounds: [f64; 4]) -> Value {
    let (shape, radius) = match node.shape {
        Shape::Rect => ("rect", 0.0),
        Shape::RoundedRect => ("rect", 0.1),
        Shape::Ellipse => ("ellipse", 0.0),
        Shape::Diamond => ("diamond", 0.0),
        Shape::Triangle => ("triangle", 0.0),
    };
    json!({
        "type":"shape", "xywh":serde_json::to_string(&bounds).unwrap(),
        "shapeType":shape, "radius":radius, "filled":true,
        "shapeStyle":match node.style {Style::General => "General", Style::Scribbled => "Scribbled"},
        "fillColor":themed(&node.fill_color,"#eef2ff","#1e293b"),
        "strokeColor":themed(&node.stroke_color,"#475569","#94a3b8"),
        "color":themed(&node.text_color,"#172554","#e2e8f0"),
        "strokeWidth":2, "strokeStyle":"solid", "roughness":1.4, "rotate":0,
        "fontFamily":"blocksuite:surface:Inter", "fontSize":node.font_size.unwrap_or(20.0),
        "fontStyle":"normal", "fontWeight":"400", "textAlign":"center",
        "textHorizontalAlign":"center", "textVerticalAlign":"center", "textResizing":1,
        "padding":[10,20], "maxWidth":false, "text":node.text
    })
}

pub fn edge_props(edge: &Edge, source: [f64; 4], target: [f64; 4]) -> Value {
    let dx = target[0] + target[2] / 2.0 - source[0] - source[2] / 2.0;
    let dy = target[1] + target[3] / 2.0 - source[1] - source[3] / 2.0;
    let facing = if dx.abs() > dy.abs() {
        if dx >= 0.0 {
            Anchor::Right
        } else {
            Anchor::Left
        }
    } else if dy >= 0.0 {
        Anchor::Bottom
    } else {
        Anchor::Top
    };
    json!({
        "type":"connector", "source":{"id":edge.source,"position":edge.source_anchor.unwrap_or(facing).position()},
        "target":{"id":edge.target,"position":edge.target_anchor.unwrap_or(facing.opposite()).position()},
        "mode":match edge.routing {Routing::Straight => 0, Routing::Orthogonal => 1, Routing::Curve => 2},
        "stroke":themed(&edge.stroke_color,"#475569","#94a3b8"), "strokeWidth":2,
        "strokeStyle":"solid", "rough":false, "roughness":1.4,
        "frontEndpointStyle":edge.start_endpoint.native(),
        "rearEndpointStyle":edge.end_endpoint.as_ref().unwrap_or(&Endpoint::Arrow).native(),
        "labelDisplay":true,"labelOffset":{"distance":0.5,"anchor":"center"},
        "labelConstraints":{"hasMaxWidth":true,"maxWidth":280},
        "labelStyle":{"color":{"light":"#172554","dark":"#e2e8f0"},
            "fontFamily":"blocksuite:surface:Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","textAlign":"center"},
        "text":edge.text
    })
}

/// Preserve each existing Y.Map and editable Y.Text, and never replace unrelated
/// surface elements. Defaults apply only to supported properties in the payload.
pub fn upsert(doc: &Doc, elements: &MapRef, id: &str, props: Value) -> Result<()> {
    let props = props.as_object().context("Invalid element properties")?;
    let kind = props
        .get("type")
        .and_then(Value::as_str)
        .context("Missing element type")?;
    ensure!(
        !props.contains_key("id") || props["id"].as_str() == Some(id),
        "Element properties.id must match its container key: {id}"
    );
    let mut txn = doc.transact_mut();
    let map = match elements.get(&txn, id) {
        Some(Out::YMap(map)) => {
            ensure!(
                map.get(&txn, "type")
                    .is_some_and(|v| v.to_string(&txn) == kind),
                "Element ID exists with a different type: {id}"
            );
            ensure!(
                !map.get(&txn, "lockedBySelf")
                    .is_some_and(|v| v.to_string(&txn) == "true"),
                "Element is locked: {id}"
            );
            ensure!(
                map.get(&txn, "id").is_none_or(
                    |v| matches!(v, Out::Any(yrs::Any::String(ref s)) if s.as_ref() == id)
                ),
                "Existing element has a conflicting internal ID: {id}"
            );
            map
        }
        Some(_) => bail!("Unsupported surface element: {id}"),
        None => {
            let index = allocate_index(&txn, elements)?;
            let map = elements.insert(&mut txn, id, MapPrelim::default());
            map.insert(&mut txn, "index", index);
            map.insert(&mut txn, "seed", 1);
            map
        }
    };
    // BlockSuite reads the model ID from this property, NOT the outer Y.Map key.
    // Upserts also heal objects written by older bridge versions without an ID.
    map.insert(&mut txn, "id", id);
    for (key, value) in props {
        if key == "text" {
            let text = value.as_str().context("Invalid label")?;
            match map.get(&txn, key) {
                Some(Out::YText(existing)) => {
                    let length = existing.len(&txn);
                    existing.remove_range(&mut txn, 0, length);
                    existing.insert(&mut txn, 0, text);
                }
                None => {
                    map.insert(&mut txn, key.as_str(), TextPrelim::new(text));
                }
                _ => bail!("Unsupported non-Y.Text label on element: {id}"),
            }
        } else {
            map.insert(
                &mut txn,
                key.as_str(),
                In::from(serde_json::from_value::<yrs::Any>(value.clone())?),
            );
        }
    }
    Ok(())
}

/// Primitives and gfx-compatible blocks share the same native layer order.
pub(crate) fn allocate_index(txn: &impl ReadTxn, elements: &MapRef) -> Result<String> {
    let mut indexes: Vec<_> = elements
        .iter(txn)
        .filter_map(|(_, value)| {
            if let Out::YMap(element) = value {
                element.get(txn, "index").map(|v| v.to_string(txn))
            } else {
                None
            }
        })
        .collect();
    if let Some(blocks) = txn.get_map("blocks") {
        indexes.extend(blocks.iter(txn).filter_map(|(_, value)| {
            if let Out::YMap(block) = value {
                block.get(txn, "prop:index").map(|v| v.to_string(txn))
            } else {
                None
            }
        }));
    }
    next_index(indexes.into_iter().max().as_deref())
}

pub fn bounds(value: &Value) -> Result<[f64; 4]> {
    let bounds: [f64; 4] =
        serde_json::from_str(value["xywh"].as_str().context("Shape has no xywh")?)?;
    ensure!(
        bounds.iter().all(|n| n.is_finite()) && bounds[2] > 0.0 && bounds[3] > 0.0,
        "Invalid existing shape bounds"
    );
    Ok(bounds)
}

pub(crate) fn document_summary(doc: &Doc) -> Result<Value> {
    use yrs::types::ToJson;
    let raw = serde_json::to_value(elements(doc)?.to_json(&doc.transact()))?;
    summary(&raw, &crate::drawing::canvas_blocks(doc)?)
}

/// Storage-level diagnostics, deliberately NOT a claim that the editor rendered.
/// Geometry excludes connector routing (computed by AFFiNE) and note blocks.
pub(crate) fn summary(raw: &Value, blocks: &[Value]) -> Result<Value> {
    let elements = raw.as_object().context("Invalid surface elements")?;
    let mut issues = Vec::new();
    let grouped = elements.values().any(|props| props["type"] == "group");
    for (id, props) in elements {
        if props.get("id").is_none() {
            let guidance = if grouped {
                "Ungroup in AFFiNE first; automatic ID repair is unavailable on grouped canvases."
            } else if matches!(
                props["type"].as_str(),
                Some("shape" | "brush" | "connector")
            ) {
                "Use affine_edit_canvas with repairMissingIds=true."
            } else {
                "Automatic ID repair is not supported for this primitive type; repair it in AFFiNE."
            };
            issues.push(json!({"code":"missing-element-id","elementId":id,
                "message":format!("AFFiNE cannot initialize this model without properties.id. {guidance}")}));
        } else if props["id"].as_str() != Some(id) {
            issues.push(json!({"code":"conflicting-element-id","elementId":id,
                "message":"properties.id differs from its container key; automatic repair will not overwrite a conflicting identity."}));
        }
    }
    let id_integrity_valid = issues.is_empty();
    let mut extent: Option<[f64; 4]> = None;
    for (id, props, key, rotation_key) in elements
        .iter()
        .map(|(id, p)| (id.as_str(), p, "xywh", "rotate"))
        .chain(blocks.iter().map(|b| {
            (
                b["id"].as_str().unwrap_or(""),
                &b["properties"],
                "prop:xywh",
                "prop:rotate",
            )
        }))
    {
        let Some(value) = props.get(key) else {
            continue;
        };
        let bound = value
            .as_str()
            .and_then(|s| serde_json::from_str::<[f64; 4]>(s).ok());
        let rotation = props.get(rotation_key).map_or(Some(0.0), Value::as_f64);
        let valid = bound.zip(rotation).filter(|(b, r)| {
            b.iter().all(|n| n.is_finite()) && b[2] > 0.0 && b[3] > 0.0 && r.is_finite()
        });
        let Some(([x, y, w, h], rotation)) = valid else {
            issues.push(json!({"code":"invalid-geometry","elementId":id,"message":"Invalid saved bounds or rotation."}));
            continue;
        };
        let (sin, cos) = rotation.to_radians().sin_cos();
        let rw = w * cos.abs() + h * sin.abs();
        let rh = w * sin.abs() + h * cos.abs();
        let b = [
            x + w / 2.0 - rw / 2.0,
            y + h / 2.0 - rh / 2.0,
            x + w / 2.0 + rw / 2.0,
            y + h / 2.0 + rh / 2.0,
        ];
        extent = Some(match extent {
            None => b,
            Some(e) => [
                e[0].min(b[0]),
                e[1].min(b[1]),
                e[2].max(b[2]),
                e[3].max(b[3]),
            ],
        });
    }
    Ok(
        json!({"elementCount":elements.len(),"canvasBlockCount":blocks.len(),
        "geometryBounds":extent.map(|b| [b[0],b[1],b[2]-b[0],b[3]-b[1]]),
        "validation":{"idIntegrityValid":id_integrity_valid,"uiVerified":false,"issues":issues},
        "viewHint":"Open this document in Edgeless mode and use Zoom to fit. An existing tab URL may override primary mode. These are persisted-data checks, not visual verification; geometryBounds excludes connector routing and notes."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_id_guidance_matches_supported_repairs_and_group_restrictions() {
        for kind in ["shape", "brush", "connector", "text", "group"] {
            let raw = json!({"missing":{"type":kind}});
            let result = summary(&raw, &[]).unwrap();
            let issue = &result["validation"]["issues"][0];
            assert_eq!(issue["code"], "missing-element-id");
            assert_eq!(issue["elementId"], "missing");
            let message = issue["message"].as_str().unwrap();
            assert!(message.contains("properties.id"));
            match kind {
                "shape" | "brush" | "connector" => {
                    assert!(message.contains("affine_edit_canvas"));
                    assert!(message.contains("repairMissingIds=true"));
                }
                "group" => {
                    assert!(!message.contains("repairMissingIds=true"));
                    assert!(message.contains("Ungroup in AFFiNE first"));
                }
                _ => {
                    assert!(!message.contains("repairMissingIds=true"));
                    assert!(message.contains("not supported for this primitive type"));
                }
            }
        }
        for kind in ["shape", "brush", "connector"] {
            let result = summary(
                &json!({
                    "missing":{"type":kind}, "group":{"type":"group","id":"group"}
                }),
                &[],
            )
            .unwrap();
            let issue = &result["validation"]["issues"][0];
            assert_eq!(issue["code"], "missing-element-id");
            assert_eq!(issue["elementId"], "missing");
            let message = issue["message"].as_str().unwrap();
            assert!(!message.contains("repairMissingIds=true"));
            assert!(message.contains("Ungroup in AFFiNE first"));
        }
    }
}
