//! Local workspace lifecycle. Existing databases are never migrated by this module.
use crate::{
    documents,
    storage::{Storage, active_pages, ensure_affine_closed, identifier, load_doc},
};
use anyhow::{Context, Result, ensure};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use chrono::Utc;
use rusqlite::{Connection, OpenFlags, backup::Backup, params};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha384};
use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
use yrs::{ArrayPrelim, Doc, Map, ReadTxn, Transact};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorkspace {
    /// Nonblank display name, at most 500 characters.
    pub name: String,
    /// Optional stable ID for retries. Must be in AFFINE_WORKSPACE_IDS when configured.
    pub workspace_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteWorkspace {
    pub workspace_id: String,
    /// Exact current display name, preventing deletion of the wrong workspace.
    pub confirm_name: String,
    /// Must be true. Moves the whole local workspace to native deleted-workspaces, not a purge.
    pub confirm: bool,
}

// Verbatim AFFiNE v0.27.4 native/schema/src/lib.rs migration SQL, with the final
// space reconstructed for migrations 1, 2 and 4. SQLx hashes raw SQL with SHA-384.
const MIGRATIONS: [(&str, &str, bool); 4] = [
    ("init_v2", include_str!("schema/1_init_v2.sql"), true),
    (
        "add_blob_sync",
        include_str!("schema/2_add_blob_sync.sql"),
        true,
    ),
    (
        "add_idx_snapshots",
        include_str!("schema/3_add_idx_snapshots.sql"),
        false,
    ),
    (
        "add_indexer_sync",
        include_str!("schema/4_add_indexer_sync.sql"),
        true,
    ),
];

fn initialize_database(id: &str, name: &str) -> Result<Connection> {
    let mut db = Connection::open_in_memory()?;
    let tx = db.transaction()?;
    tx.execute_batch(
        "CREATE TABLE _sqlx_migrations (
        version BIGINT PRIMARY KEY,
        description TEXT NOT NULL,
        installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
        success BOOLEAN NOT NULL,
        checksum BLOB NOT NULL,
        execution_time BIGINT NOT NULL
    );",
    )?;
    for (index, (description, sql, trailing_space)) in MIGRATIONS.iter().enumerate() {
        let sql = if *trailing_space {
            format!("{}\n ", sql.trim_end_matches('\n'))
        } else {
            (*sql).to_owned()
        };
        tx.execute_batch(&sql)?;
        tx.execute("INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES (?1,?2,1,?3,0)", params![index as i64 + 1, description, Sha384::digest(sql.as_bytes()).as_slice()])?;
    }
    tx.execute("INSERT INTO meta(space_id) VALUES (?1)", [id])?;
    let root = Doc::new();
    let meta = root.get_or_insert_map("meta");
    {
        let mut txn = root.transact_mut();
        meta.insert(&mut txn, "name", name);
        meta.insert(&mut txn, "pages", ArrayPrelim::default());
    }
    let bytes = root
        .transact()
        .encode_state_as_update_v1(&Default::default());
    let timestamp = Utc::now()
        .naive_utc()
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string();
    tx.execute(
        "INSERT INTO snapshots(doc_id,data,updated_at) VALUES (?1,?2,?3)",
        params![id, bytes, timestamp],
    )?;
    tx.execute(
        "INSERT INTO clocks(doc_id,timestamp) VALUES (?1,?2)",
        params![id, timestamp],
    )?;
    tx.commit()?;
    validate_workspace(&db, id, name)?;
    Ok(db)
}

fn validate_workspace(db: &Connection, id: &str, expected_name: &str) -> Result<()> {
    let integrity: String = db.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    ensure!(integrity == "ok", "Workspace integrity check failed");
    let space_id: String = db.query_row("SELECT space_id FROM meta", [], |row| row.get(0))?;
    ensure!(space_id == id, "Workspace directory and space_id disagree");
    let root = documents::root_json(&load_doc(db, id)?, "meta")?;
    ensure!(
        root["name"] == expected_name,
        "Workspace name does not match confirmName/requested name"
    );
    active_pages(&root)?;
    Ok(())
}

fn exists(dir: &Dir, path: &str) -> Result<bool> {
    match dir.symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Reject symlinked managed subdirectories before creating anything beneath them.
fn managed_directory(data_dir: &Path, parts: &[&str]) -> Result<(Dir, PathBuf)> {
    fs::create_dir_all(data_dir)?;
    let mut path = data_dir.canonicalize()?;
    let mut dir = Dir::open_ambient_dir(&path, ambient_authority())?;
    for part in parts {
        path.push(part);
        match dir.symlink_metadata(part) {
            Ok(meta) => ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "Managed directory must not be a symlink: {}",
                path.display()
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => dir.create_dir(part)?,
            Err(error) => return Err(error.into()),
        }
        dir = dir.open_dir(part)?;
    }
    Ok((dir, path))
}

fn backup(db: &Connection, data_dir: &Path, id: &str) -> Result<PathBuf> {
    let (dir, path) = managed_directory(data_dir, &["mcp-backups"])?;
    let file_name = format!("{id}-{}.db", Uuid::new_v4());
    write_database(db, &dir, &file_name)?;
    Ok(path.join(file_name))
}

// Back up WAL content through SQLite, then write through a directory capability,
// never an ambient path that could be redirected outside the configured root.
fn write_database(db: &Connection, dir: &Dir, name: &str) -> Result<()> {
    let mut copy = Connection::open_in_memory()?;
    Backup::new(db, &mut copy)?.run_to_completion(100, Duration::from_millis(10), None)?;
    let bytes = copy.serialize("main")?;
    let mut file = dir.open_with(name, OpenOptions::new().write(true).create_new(true))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    ensure!(
        dir.read(name)? == *bytes,
        "Workspace database file readback failed"
    );
    Ok(())
}

fn lifecycle_lock(data_dir: &Path) -> Result<fs::File> {
    let (dir, _) = managed_directory(data_dir, &[])?;
    let file = dir
        .open_with(
            "mcp-workspace-lifecycle.lock",
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false),
        )?
        .into_std();
    file.lock()?;
    Ok(file)
}

/// Atomic directory publication/archive with no replacement, even when a destination
/// appears after preflight. Source and destination names are single validated segments.
fn rename_directory_no_replace(from: &Dir, source: &str, to: &Dir, target: &str) -> Result<()> {
    ensure!(
        !source.is_empty()
            && !target.is_empty()
            && !source.contains('/')
            && !target.contains('/')
            && !source.contains('\\')
            && !target.contains('\\'),
        "Invalid directory rename segment"
    );
    #[cfg(unix)]
    {
        use std::{ffi::CString, os::fd::AsRawFd};
        let source = CString::new(source)?;
        let target = CString::new(target)?;
        // SAFETY: both directory descriptors stay alive through the call and both
        // NUL-terminated names are validated single relative segments.
        #[cfg(target_os = "macos")]
        let result = unsafe {
            libc::renameatx_np(
                from.as_raw_fd(),
                source.as_ptr(),
                to.as_raw_fd(),
                target.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                from.as_raw_fd(),
                source.as_ptr(),
                to.as_raw_fd(),
                target.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        anyhow::bail!("Workspace lifecycle is supported only on macOS, Linux and Windows");
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        ensure!(
            result == 0,
            "Atomic directory rename failed: {}",
            std::io::Error::last_os_error()
        );
    }
    // Ordinary Windows rename also replaces empty directories. The NT API supports
    // directory-relative destination handles; the Win32 wrapper rejects this form.
    #[cfg(windows)]
    {
        use cap_std::fs::OpenOptionsExt;
        use std::{
            mem::{offset_of, size_of},
            os::windows::io::AsRawHandle,
        };
        use windows_sys::{
            Wdk::Storage::FileSystem::{
                FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
            },
            Win32::{
                Foundation::RtlNtStatusToDosError,
                Storage::FileSystem::{
                    DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
                    FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
                },
                System::IO::IO_STATUS_BLOCK,
            },
        };

        let directory = from.open_with(
            source,
            OpenOptions::new()
                .access_mode(DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS),
        )?;
        let name: Vec<u16> = target.encode_utf16().chain([0]).collect();
        let bytes = offset_of!(FILE_RENAME_INFORMATION, FileName) + name.len() * size_of::<u16>();
        // usize storage supplies the alignment required by the structure's HANDLE.
        let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        let mut io_status = IO_STATUS_BLOCK::default();
        // SAFETY: the aligned allocation holds the complete struct and UTF-16 name.
        // Both handles remain alive, the destination is a single relative segment,
        // and FileNameLength excludes the terminating NUL as required by the NT API.
        let result = unsafe {
            std::ptr::write(info, FILE_RENAME_INFORMATION::default());
            (*info).Anonymous.ReplaceIfExists = false;
            (*info).RootDirectory = to.as_raw_handle();
            (*info).FileNameLength = ((name.len() - 1) * size_of::<u16>()).try_into()?;
            std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
            NtSetInformationFile(
                directory.as_raw_handle(),
                &mut io_status,
                info.cast(),
                bytes.try_into()?,
                FileRenameInformation,
            )
        };
        ensure!(
            result >= 0,
            "Atomic directory rename failed: {}",
            // SAFETY: converting a returned NTSTATUS has no pointer arguments.
            std::io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(result) } as i32)
        );
    }
    Ok(())
}

impl Storage {
    fn workspace_write_allowed(&self, id: &str) -> Result<()> {
        ensure!(
            self.writes,
            "Writes disabled. Set AFFINE_ENABLE_WRITES=1 and quit AFFiNE Desktop before writing."
        );
        ensure!(identifier(id), "Invalid workspaceId");
        ensure!(
            self.allowlist.as_ref().is_none_or(|ids| ids.contains(id)),
            "Unknown or disallowed local workspace: {id}"
        );
        ensure_affine_closed()
    }

    pub fn create_workspace(&self, p: &CreateWorkspace) -> Result<Value> {
        ensure!(
            !p.name.trim().is_empty() && p.name.chars().count() <= 500,
            "name must contain 1..500 characters"
        );
        let id = p
            .workspace_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        self.workspace_write_allowed(&id)?;
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Write lock poisoned"))?;
        ensure_affine_closed()?;
        let _process_guard = lifecycle_lock(&self.data_dir)?;
        let (root, _) = managed_directory(&self.data_dir, &["workspaces", "local"])?;
        let (data, _) = managed_directory(&self.data_dir, &[])?;
        ensure!(
            !exists(&data, &format!("deleted-workspaces/{id}"))?,
            "workspaceId belongs to a deleted workspace; choose a new ID or recover it in AFFiNE"
        );
        ensure!(
            !exists(&data, &format!("workspaces/{id}"))?,
            "Legacy workspace data exists for this ID; use AFFiNE to manage it"
        );
        if exists(&root, &id)? {
            let meta = root.symlink_metadata(&id)?;
            ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "Workspace path must be a real directory"
            );
            let db = self.open(&id, false)?;
            validate_workspace(&db, &id, p.name.trim())?;
            let backup_path = backup(&db, &self.data_dir, &id)?;
            return Ok(
                json!({"workspaceId":id,"name":p.name.trim(),"alreadyExists":true,"backupPath":backup_path,"reopenAffine":true}),
            );
        }
        // Hidden UUID staging never reserves the requested ID. If the process dies,
        // a retry can create a new stage without deleting or trusting orphan files.
        let stage_name = format!(".mcp-create-{}", Uuid::new_v4());
        let (staging_root, staging_path) = managed_directory(&self.data_dir, &["mcp-staging"])?;
        staging_root.create_dir(&stage_name)?;
        let stage = staging_root.open_dir(&stage_name)?;
        let db = initialize_database(&id, p.name.trim())?;
        write_database(&db, &stage, "storage.db")?;
        let mut persisted = Connection::open_in_memory()?;
        let file = stage.open("storage.db")?;
        let size = file.metadata()?.len() as usize;
        persisted.deserialize_read_exact("main", file, size, true)?;
        validate_workspace(&persisted, &id, p.name.trim())?;
        let backup_path = backup(&db, &self.data_dir, &id)?;
        ensure_affine_closed()?;
        drop(stage); // Release Windows handles before the directory move.
        rename_directory_no_replace(&staging_root, &stage_name, &root, &id).with_context(|| {
            format!(
                "Workspace not published; staging retained at {}",
                staging_path.join(&stage_name).display()
            )
        })?;
        Ok(
            json!({"workspaceId":id,"name":p.name.trim(),"alreadyExists":false,"backupPath":backup_path,"reopenAffine":true}),
        )
    }

    pub fn delete_workspace(&self, p: &DeleteWorkspace) -> Result<Value> {
        ensure!(
            p.confirm,
            "Workspace deletion requires confirm=true and exact confirmName"
        );
        self.workspace_write_allowed(&p.workspace_id)?;
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Write lock poisoned"))?;
        ensure_affine_closed()?;
        let _process_guard = lifecycle_lock(&self.data_dir)?;
        let (root, _) = managed_directory(&self.data_dir, &["workspaces", "local"])?;
        let (deleted_root, deleted_path) =
            managed_directory(&self.data_dir, &["deleted-workspaces"])?;
        let deleted = deleted_path.join(&p.workspace_id);
        let (data, _) = managed_directory(&self.data_dir, &[])?;
        ensure!(
            !exists(&data, &format!("workspaces/{}", p.workspace_id))?,
            "Legacy workspace data exists; delete this workspace with AFFiNE to avoid partial removal"
        );
        if !exists(&root, &p.workspace_id)? && exists(&deleted_root, &p.workspace_id)? {
            let meta = deleted_root.symlink_metadata(&p.workspace_id)?;
            ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "Deleted workspace path must be a real directory"
            );
            let path = deleted.join("storage.db");
            ensure!(
                !fs::symlink_metadata(&path)?.file_type().is_symlink(),
                "Database symlinks cannot be managed"
            );
            let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            validate_workspace(&db, &p.workspace_id, &p.confirm_name)?;
            return Ok(
                json!({"workspaceId":p.workspace_id,"alreadyDeleted":true,"deletedWorkspacePath":deleted,"backupPath":path,"reopenAffine":true}),
            );
        }
        ensure!(
            !exists(&deleted_root, &p.workspace_id)?,
            "A deleted-workspace backup already exists for this ID; nothing was overwritten"
        );
        let folder_meta = root
            .symlink_metadata(&p.workspace_id)
            .context("Unknown local workspace")?;
        ensure!(
            folder_meta.is_dir() && !folder_meta.file_type().is_symlink(),
            "Workspace path must be a real directory"
        );
        ensure!(
            !root
                .symlink_metadata(format!("{}/storage.db", p.workspace_id))?
                .file_type()
                .is_symlink(),
            "Database symlinks cannot be managed"
        );
        let db = self.open(&p.workspace_id, false)?;
        validate_workspace(&db, &p.workspace_id, &p.confirm_name)?;
        let backup_path = backup(&db, &self.data_dir, &p.workspace_id)?;
        let saved = Connection::open_with_flags(&backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        validate_workspace(&saved, &p.workspace_id, &p.confirm_name)?;
        drop(saved);
        drop(db); // Windows must release the connection before moving the entire folder.
        ensure_affine_closed()?;
        rename_directory_no_replace(&root, &p.workspace_id, &deleted_root, &p.workspace_id)
            .context(
                "Could not move workspace to native deleted-workspaces; original data retained",
            )?;
        ensure!(
            !exists(&root, &p.workspace_id)?
                && deleted_root
                    .metadata(format!("{}/storage.db", p.workspace_id))?
                    .is_file(),
            "Workspace deletion readback failed"
        );
        Ok(
            json!({"workspaceId":p.workspace_id,"alreadyDeleted":false,"deletedWorkspacePath":deleted,"backupPath":backup_path,"reopenAffine":true}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_publication_and_archival_never_replace_a_target() {
        let data = tempfile::tempdir().unwrap();
        let (dir, _) = managed_directory(data.path(), &[]).unwrap();
        dir.create_dir("source").unwrap();
        dir.create_dir("target").unwrap();
        // Empty destinations must also be protected, unlike ordinary Unix rename.
        assert!(rename_directory_no_replace(&dir, "source", &dir, "target").is_err());
        assert!(dir.metadata("source").unwrap().is_dir());
        assert!(dir.metadata("target").unwrap().is_dir());
        rename_directory_no_replace(&dir, "source", &dir, "new-target").unwrap();
        assert!(!exists(&dir, "source").unwrap());
        assert!(dir.metadata("new-target").unwrap().is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn directory_handles_remain_anchored_after_path_replacement() {
        let data = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (dir, path) = managed_directory(data.path(), &["workspaces", "local"]).unwrap();
        fs::rename(&path, data.path().join("original-local")).unwrap();
        std::os::unix::fs::symlink(outside.path(), &path).unwrap();
        dir.create_dir("stage").unwrap();
        let stage = dir.open_dir("stage").unwrap();
        let db = initialize_database("protected", "Protected").unwrap();
        write_database(&db, &stage, "storage.db").unwrap();
        drop(stage);
        rename_directory_no_replace(&dir, "stage", &dir, "protected").unwrap();
        assert!(
            data.path()
                .join("original-local/protected/storage.db")
                .is_file()
        );
        assert!(!outside.path().join("protected").exists());
        assert!(!outside.path().join("stage").exists());
    }

    #[test]
    fn initialized_database_has_complete_native_schema_and_migration_records() {
        let db = initialize_database("workspace-test", "Test").unwrap();
        // Independently retrieved SHA-384 digests of AFFiNE v0.27.4's raw migration SQL.
        let upstream_checksums = [
            "a1f0a1496ba1d1ff1689fc234514b13e7501ce5a3891b5943a75300b20e68444444ae71a1a80f40e46ccee2fc9e2af1a",
            "c40244fec04822d74db419bead8486db435bb52d1f0214e4ddcc8928f4444475c7be8f96cee4c4383fd21b866fffd630",
            "c13e51745e6f2d3e49f01fc82df68e88e91feabfa0a28507d658bb85b4c5cb81354ac8b23eb1038b175b1ae66311980a",
            "eeb9b2d07c3827f326feaed6651f587f177c2312c701d72f321c2d1a132bcd962fc914b39b12a34694003033bf29882b",
        ];
        for (index, (description, sql, trailing_space)) in MIGRATIONS.iter().enumerate() {
            let sql = if *trailing_space {
                format!("{}\n ", sql.trim_end_matches('\n'))
            } else {
                (*sql).to_owned()
            };
            assert_eq!(
                Sha384::digest(sql.as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                upstream_checksums[index]
            );
            let (stored_name, success, checksum): (String, bool, Vec<u8>) = db
                .query_row(
                    "SELECT description,success,checksum FROM _sqlx_migrations WHERE version=?1",
                    [index as i64 + 1],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(stored_name, *description);
            assert!(success);
            assert_eq!(checksum, Sha384::digest(sql.as_bytes()).as_slice());
        }
        for table in [
            "meta",
            "snapshots",
            "updates",
            "clocks",
            "blobs",
            "peer_clocks",
            "peer_blob_sync",
            "idx_snapshots",
            "indexer_sync",
        ] {
            db.prepare(&format!("SELECT * FROM {table}")).unwrap();
        }
        assert_eq!(
            active_pages(
                &documents::root_json(&load_doc(&db, "workspace-test").unwrap(), "meta").unwrap()
            )
            .unwrap()
            .len(),
            0
        );
    }
}
