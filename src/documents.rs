use anyhow::{Result, bail};
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
use yrs::{Doc, ReadTxn, Transact, types::ToJson};

pub fn root_json(doc: &Doc, name: &str) -> Result<Value> {
    let txn = doc.transact();
    let map = txn
        .get_map(name)
        .ok_or_else(|| anyhow::anyhow!("Missing Yjs map: {name}"))?;
    Ok(serde_json::to_value(map.to_json(&txn))?)
}

fn unwrap(mut value: &Value) -> &Value {
    while value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|s| s.starts_with("$blocksuite:"))
    {
        match value.get("value") {
            Some(inner) => value = inner,
            None => break,
        }
    }
    value
}

pub fn text(value: &Value) -> String {
    let value = unwrap(value);
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Array(items) => items
            .iter()
            .map(|item| text(item.get("insert").unwrap_or(item)))
            .collect(),
        Value::Object(_) => value.get("delta").map(text).unwrap_or_default(),
        _ => String::new(),
    }
}

fn prop(block: &Value, key: &str) -> String {
    text(&block[key])
}

fn table(block: &Value) -> String {
    let Some(fields) = block.as_object() else {
        return String::new();
    };
    let ordered = |prefix: &str| {
        let mut ids: Vec<_> = fields
            .iter()
            .filter_map(|(key, value)| {
                key.strip_prefix(prefix)?
                    .strip_suffix(".order")
                    .map(|id| (text(value), id.to_owned()))
            })
            .collect();
        ids.sort();
        ids.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
    };
    let rows = ordered("prop:rows.");
    let columns = ordered("prop:columns.");
    if rows.is_empty() || columns.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let cells: Vec<_> = columns
            .iter()
            .map(|column| {
                prop(block, &format!("prop:cells.{row}:{column}.text"))
                    .replace('|', "\\|")
                    .replace('\n', "<br>")
            })
            .collect();
        lines.push(format!("| {} |", cells.join(" | ")));
        if index == 0 {
            lines.push(format!("| {} |", vec!["---"; columns.len()].join(" | ")));
        }
    }
    lines.join("\n")
}

fn surface(block: &Value) -> String {
    let Some(elements) = unwrap(&block["prop:elements"]).as_object() else {
        return String::new();
    };
    let mut elements: Vec<_> = elements.iter().collect();
    elements.sort_by_key(|(id, item)| (prop(item, "index"), *id));
    elements
        .into_iter()
        .filter_map(|(id, element)| {
            let body = ["text", "title", "label"]
                .iter()
                .map(|key| prop(element, key))
                .find(|s| !s.is_empty())?;
            Some(format!("[Canvas {} {id}] {body}", prop(element, "type")))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn content(block: &Value, warnings: &mut BTreeSet<String>) -> String {
    let flavour = prop(block, "sys:flavour");
    let body = prop(block, "prop:text");
    let title = prop(block, "prop:title");
    match flavour.as_str() {
        "affine:page" | "affine:note" | "affine:edgeless-text" => String::new(),
        "affine:paragraph" => {
            let kind = prop(block, "prop:type");
            match kind.as_str() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => format!(
                    "{} {body}",
                    "#".repeat(kind.as_bytes()[1] as usize - b'0' as usize)
                ),
                "quote" => body
                    .lines()
                    .map(|line| format!("> {line}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => body,
            }
        }
        "affine:list" => {
            let prefix = match prop(block, "prop:type").as_str() {
                "todo" if block["prop:checked"].as_bool().unwrap_or(false) => "- [x]",
                "todo" => "- [ ]",
                "numbered" => "1.",
                _ => "-",
            };
            format!("{prefix} {body}")
        }
        "affine:code" => {
            let longest = body
                .split(|c| c != '`')
                .map(str::len)
                .max()
                .unwrap_or(0)
                .max(2);
            let fence = "`".repeat(longest + 1);
            let language: String = prop(block, "prop:language")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || "_+-".contains(*c))
                .collect();
            format!("{fence}{language}\n{body}\n{fence}")
        }
        "affine:divider" => "---".into(),
        "affine:table" => table(block),
        "affine:surface" => surface(block),
        "affine:frame" => {
            if title.is_empty() {
                title
            } else {
                format!("## {title}")
            }
        }
        "affine:latex" => format!("$$\n{}\n$$", prop(block, "prop:latex")),
        "affine:image" | "affine:attachment" => {
            warnings.insert(
                "Images/attachments have labels only; no OCR or binary content is returned.".into(),
            );
            format!(
                "[{}: {}; blob: {}]",
                flavour,
                prop(block, "prop:caption"),
                prop(block, "prop:sourceId")
            )
        }
        "affine:database" => {
            warnings.insert("Database row text is included; typed properties and view layouts are not fully rendered.".into());
            format!("## {title}")
        }
        _ if flavour == "affine:bookmark" || flavour.starts_with("affine:embed-") => {
            let url = prop(block, "prop:url");
            let page = prop(block, "prop:pageId");
            if !url.is_empty() {
                format!("[{title}]({url})")
            } else if !page.is_empty() {
                format!("[Embedded document: {page}]")
            } else {
                title
            }
        }
        _ => {
            warnings.insert(format!(
                "Best-effort extraction for unsupported block: {flavour}"
            ));
            format!("{title}\n{body}").trim().to_owned()
        }
    }
}

#[derive(Debug)]
pub struct Rendered {
    pub title: String,
    pub markdown: String,
    pub block_count: usize,
    pub warnings: Vec<String>,
}

pub fn render(doc: &Doc) -> Result<Rendered> {
    let blocks = root_json(doc, "blocks")?;
    let blocks = blocks
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Invalid BlockSuite blocks"))?;
    let pages: Vec<_> = blocks
        .iter()
        .filter(|(_, block)| block["sys:flavour"] == "affine:page")
        .collect();
    if pages.len() != 1 {
        bail!("Unsupported document: expected one AFFiNE page root")
    }
    let (root, page) = pages[0];
    let mut title = prop(page, "prop:title").trim().to_owned();
    if title.is_empty() {
        title = "Untitled".into();
    }
    let mut output = vec![format!("# {title}")];
    let mut warnings = BTreeSet::new();
    let mut visited = HashSet::new();
    let mut stack = vec![(root.as_str(), 0usize)];
    while let Some((id, depth)) = stack.pop() {
        if !visited.insert(id) {
            warnings.insert("Duplicate/cyclic block references were skipped.".into());
            continue;
        }
        let Some(block) = blocks.get(id) else {
            warnings.insert(format!("Missing block: {id}"));
            continue;
        };
        let body = content(block, &mut warnings);
        if !body.is_empty() {
            output.push(body);
        }
        if let Some(children) = block["sys:children"].as_array() {
            if depth >= 100 {
                if !children.is_empty() {
                    warnings.insert("Nesting beyond 100 levels was omitted.".into());
                }
            } else {
                stack.extend(
                    children
                        .iter()
                        .rev()
                        .filter_map(|child| child.as_str().map(|id| (id, depth + 1))),
                );
            }
        }
    }
    Ok(Rendered {
        title,
        markdown: output.join("\n\n"),
        block_count: visited.len(),
        warnings: warnings.into_iter().collect(),
    })
}
