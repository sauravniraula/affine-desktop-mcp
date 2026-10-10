//! Offline, content-addressed media imports into AFFiNE's native blob table.
use crate::{canvas::EditCanvas, drawing};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::{Connection, OptionalExtension, params};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

const MAX_FILE_BYTES: usize = 20 * 1024 * 1024;
const MAX_BATCH_BYTES: usize = 100 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Auto,
    Image,
    Attachment,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Media {
    pub id: String,
    /// File inside <AFFINE_DATA_DIR>/mcp-imports. Absolute or import-folder-relative path.
    /// Symlinks escaping that folder are rejected. Maximum 20 MiB per file.
    pub file_path: String,
    pub position: [f64; 2],
    /// Optional display [width,height]. Image defaults fit within 640x480;
    /// attachments default to a 460x80 card.
    pub size: Option<[f64; 2]>,
    /// auto detects supported raster images; other files become attachments.
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub caption: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportMedia {
    pub workspace_id: String,
    pub document_id: String,
    pub media: Vec<Media>,
}
impl ImportMedia {
    pub fn into_edit(self) -> EditCanvas {
        EditCanvas {
            workspace_id: self.workspace_id,
            document_id: self.document_id,
            media: self.media,
            ..Default::default()
        }
    }
}

pub(crate) struct Prepared {
    pub id: String,
    pub source_id: String,
    pub data: Vec<u8>,
    pub mime: String,
    pub flavour: &'static str,
    pub props: Value,
}
impl Prepared {
    pub fn result(&self) -> Value {
        json!({"blockId":self.id,"sourceId":self.source_id,"mimeType":self.mime,
            "sizeBytes":self.data.len(),"kind":if self.flavour == "affine:image" {"image"} else {"attachment"}})
    }
}

pub(crate) fn validate(items: &[Media]) -> Result<()> {
    ensure!(items.len() <= 20, "At most 20 media files per call");
    for item in items {
        ensure!(
            !item.file_path.is_empty() && item.file_path.len() <= 4096,
            "Invalid media filePath"
        );
        ensure!(
            item.caption.len() <= 2000,
            "Media caption must be at most 2000 UTF-8 bytes"
        );
        drawing::validate_position(item.position)?;
        if let Some(size) = item.size {
            drawing::validate_size(size)?;
        }
    }
    Ok(())
}

fn image_mime(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

/// Read only a deliberately exposed import folder, never arbitrary files, network
/// URLs, or a file path contained in untrusted document text.
pub(crate) fn prepare(data_dir: &Path, items: &[Media]) -> Result<Vec<Prepared>> {
    validate(items)?;
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let data_root = data_dir
        .canonicalize()
        .context("Missing AFFiNE data directory")?;
    let root = data_dir
        .join("mcp-imports")
        .canonicalize()
        .context("Place media in <AFFINE_DATA_DIR>/mcp-imports before importing")?;
    ensure!(
        root.starts_with(&data_root),
        "Media import folder must not escape the AFFiNE data directory"
    );
    let mut total_bytes = 0usize;
    let mut result = Vec::new();
    for item in items {
        let requested = Path::new(&item.file_path);
        let path = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            root.join(requested)
        };
        let path = path
            .canonicalize()
            .with_context(|| format!("Cannot open media file: {}", item.file_path))?;
        ensure!(
            path.starts_with(&root),
            "Media file must be inside {}",
            root.display()
        );
        let file = File::open(&path)?;
        let metadata = file.metadata()?;
        ensure!(metadata.is_file(), "Media import requires a regular file");
        ensure!(
            metadata.len() > 0 && metadata.len() <= MAX_FILE_BYTES as u64,
            "Media files must contain 1 byte..20 MiB"
        );
        let mut data = Vec::new();
        file.take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut data)?;
        ensure!(
            !data.is_empty() && data.len() <= MAX_FILE_BYTES,
            "Media file changed or exceeds 20 MiB"
        );
        total_bytes += data.len();
        ensure!(
            total_bytes <= MAX_BATCH_BYTES,
            "Media batch exceeds 100 MiB"
        );
        let image_type = image_mime(&data);
        let as_image = match item.kind {
            Kind::Auto => image_type.is_some(),
            Kind::Image => {
                ensure!(
                    image_type.is_some(),
                    "Native image import supports PNG, JPEG, GIF and WebP; use attachment for other formats"
                );
                true
            }
            Kind::Attachment => false,
        };
        let mime = image_type.map(str::to_owned).unwrap_or_else(|| {
            mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string()
        });
        let source_id = URL_SAFE_NO_PAD.encode(Sha256::digest(&data));
        let name = path
            .file_name()
            .context("Media has no filename")?
            .to_string_lossy()
            .into_owned();
        ensure!(name.len() <= 500, "Media filename is too long");
        let (flavour, size, mut props) = if as_image {
            let natural =
                imagesize::blob_size(&data).context("Invalid or unsupported image header")?;
            ensure!(
                natural.width > 0
                    && natural.height > 0
                    && natural.width <= 20_000
                    && natural.height <= 20_000
                    && natural.width.saturating_mul(natural.height) <= 100_000_000,
                "Image dimensions are too large"
            );
            let scale = (640.0 / natural.width as f64)
                .min(480.0 / natural.height as f64)
                .min(1.0);
            let size = item
                .size
                .unwrap_or([natural.width as f64 * scale, natural.height as f64 * scale]);
            (
                "affine:image",
                size,
                json!({"sourceId":source_id,"caption":item.caption,
                "width":natural.width,"height":natural.height,"size":data.len(),"rotate":0}),
            )
        } else {
            (
                "affine:attachment",
                item.size.unwrap_or([460.0, 80.0]),
                json!({"sourceId":source_id,"name":name,"type":mime,"size":data.len(),"caption":item.caption,
                    "embed":false,"style":"horizontalThin","rotate":0,"footnoteIdentifier":null}),
            )
        };
        let [x, y] = item.position;
        props["xywh"] = json!(serde_json::to_string(&[x, y, size[0], size[1]])?);
        result.push(Prepared {
            id: item.id.clone(),
            source_id,
            data,
            mime,
            flavour,
            props,
        });
    }
    Ok(result)
}

/// Participate in the caller's SQLite transaction, so blob + block + registry
/// writes all roll back together. Never delete shared blobs when deleting blocks.
pub(crate) fn persist(db: &Connection, items: &[Prepared]) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let columns = db
        .prepare("PRAGMA table_info(blobs)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
    ensure!(
        ["key", "data", "mime", "size", "created_at", "deleted_at"]
            .iter()
            .all(|key| columns.contains(*key)),
        "Unsupported AFFiNE blob storage schema"
    );
    for item in items {
        let existing: Option<(Vec<u8>, String)> = db
            .query_row(
                "SELECT data,mime FROM blobs WHERE key=?",
                [&item.source_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((data, mime)) = existing {
            ensure!(
                data == item.data && mime == item.mime,
                "Existing blob conflicts with imported content or MIME type"
            );
            db.execute(
                "UPDATE blobs SET deleted_at=NULL WHERE key=?",
                [&item.source_id],
            )?;
        } else {
            db.execute(
                "INSERT INTO blobs(key,data,mime,size) VALUES (?1,?2,?3,?4)",
                params![item.source_id, item.data, item.mime, item.data.len() as i64],
            )?;
        }
        let saved: (Vec<u8>, String, i64) = db.query_row(
            "SELECT data,mime,size FROM blobs WHERE key=? AND deleted_at IS NULL",
            [&item.source_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        ensure!(
            saved.0 == item.data && saved.1 == item.mime && saved.2 == item.data.len() as i64,
            "Media blob readback failed"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn attachment_import_is_content_addressed_and_rejects_escaping_paths() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        fs::create_dir_all(data.join("mcp-imports")).unwrap();
        fs::write(
            data.join("mcp-imports/example.txt"),
            b"Hello local attachment",
        )
        .unwrap();
        let item: Media =
            serde_json::from_value(json!({"id":"a","filePath":"example.txt","position":[100,200]}))
                .unwrap();
        let a = prepare(&data, &[item]).unwrap().pop().unwrap();
        let item: Media =
            serde_json::from_value(json!({"id":"b","filePath":"example.txt","position":[300,200]}))
                .unwrap();
        let b = prepare(&data, &[item]).unwrap().pop().unwrap();
        assert_eq!(a.source_id, b.source_id);
        assert_eq!(a.flavour, "affine:attachment");
        assert_eq!(a.mime, "text/plain");
        fs::write(data.join("outside.txt"), b"Must not import").unwrap();
        let bad: Media = serde_json::from_value(
            json!({"id":"bad","filePath":"../outside.txt","position":[0,0]}),
        )
        .unwrap();
        assert!(prepare(&data, &[bad]).is_err());
        let image: Media = serde_json::from_value(
            json!({"id":"bad","filePath":"example.txt","position":[0,0],"kind":"image"}),
        )
        .unwrap();
        assert!(prepare(&data, &[image]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn media_symlinks_cannot_escape_the_import_folder() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        fs::create_dir_all(data.join("mcp-imports")).unwrap();
        fs::write(dir.path().join("outside.txt"), b"Outside").unwrap();
        std::os::unix::fs::symlink(
            dir.path().join("outside.txt"),
            data.join("mcp-imports/link.txt"),
        )
        .unwrap();
        let item: Media =
            serde_json::from_value(json!({"id":"bad","filePath":"link.txt","position":[0,0]}))
                .unwrap();
        assert!(prepare(&data, &[item]).is_err());
    }
}
