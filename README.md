# AFFiNE Desktop local MCP bridge

A **Rust stdio MCP server for AFFiNE Desktop offline/local-only workspaces**.
It reads the desktop application's local SQLite/Yjs storage, creates normal
pages and Edgeless canvases, and creates/edits **native editable diagrams** with
shapes, rich-text labels, bound connectors, pen strokes, frames, link cards,
free-floating curves, images and file attachments.

**No AFFiNE Cloud, Docker/self-hosted AFFiNE server, GraphQL API, cloud login,
or network listener is used.** Docker is used only as an optional Linux test
runner during development, not by the application.

## Important boundary

- Reads can run while AFFiNE Desktop is open, but see only **persisted** content.
- Writes are disabled by default. Enable them explicitly and **quit AFFiNE Desktop
  completely before creating or editing documents**. Reopen AFFiNE after changes.
- This is a storage bridge, **not a live IPC connection to the running editor**.
  AFFiNE's own write path emits in-process update events that an external process
  cannot deliver. The bridge refuses writes when an AFFiNE process is detected.
  Keep the application closed for the entire write operation; do not open it
  concurrently. Renamed/custom-packaged executables may evade name detection.
- Writes append Yjs updates; they do **not** overwrite existing snapshots or
  replace entire workspace metadata. Canvas editing preserves unrelated surface
  elements and note blocks; general page-body editing is not implemented.
- Tested against **AFFiNE Desktop 0.27.4**. Storage is an internal AFFiNE contract,
  not a stable public API. Other versions are not guaranteed; missing required
  schema fields and incomplete Yjs updates fail rather than silently returning
  partial documents.

## Build and run

Install Rust **1.95 or later** and a C compiler (SQLite is bundled):

- macOS: Xcode Command Line Tools.
- Windows: Rust's MSVC toolchain and Visual Studio Build Tools with Desktop
  development with C++.
- Linux: a C toolchain such as `build-essential` on Debian/Ubuntu.

```sh
cargo build --release --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

macOS/Linux:

```sh
./target/release/affine-local-mcp --check
./target/release/affine-local-mcp
```

Windows PowerShell:

```powershell
.\target\release\affine-local-mcp.exe --check
.\target\release\affine-local-mcp.exe
```

The normal invocation waits for MCP requests on stdin. Stdout is reserved for MCP
JSON messages. `--check`, `--help` and `--version` are standalone diagnostics.

Alternatively, install the binary to your Cargo bin directory:

```sh
cargo install --path . --locked
```

## Releases

The tag-triggered release workflow builds native **amd64 (x86_64)** and
**arm64 (aarch64)** binaries for Windows, macOS and Linux. Windows downloads
are ZIP archives; macOS and Linux downloads are `.tar.gz` archives. Each
archive includes the executable, this README and the Apache 2.0 license.
The release also includes `SHA256SUMS` for verifying downloads.
Linux binaries are built on Ubuntu 22.04 and require glibc 2.35 or later.

To release, set `[package].version` in `Cargo.toml`, update `Cargo.lock` with
`cargo check`, commit the changes, then push a matching tag such as `v0.1.0`.
Unprefixed tags such as `0.1.0` are also accepted. The tag's version must match
the manifest exactly, including any prerelease/build suffix.

Before any build, the workflow skips publication if the tag does not match
the manifest or a release already exists for that version (with or without
the `v` prefix, including drafts and prereleases). GitHub API errors fail the
check rather than being treated as permission to release. Publication runs
only after all six builds and tests succeed, and checks for an existing release
again before uploading. Prerelease versions are marked as GitHub prereleases.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).

## Desktop data directories

| OS | Default application data directory |
| --- | --- |
| macOS | `~/Library/Application Support/AFFiNE` |
| Windows | `%APPDATA%\AFFiNE` (fallback: `%USERPROFILE%\AppData\Roaming\AFFiNE`) |
| Linux | `$XDG_CONFIG_HOME/AFFiNE` (fallback: `~/.config/AFFiNE`) |

Only `workspaces/local/<workspace-id>/storage.db` beneath this directory is
considered. Cloud caches, userspaces, trashed documents and unregistered leftover
documents are excluded. Symlinked workspace directories and database paths
escaping the configured root are not exposed.

For Flatpak, portable, beta, renamed, or custom installations, set
`AFFINE_DATA_DIR` to the directory **containing `workspaces`**, not to a database
file. You can also pass `--data-dir PATH`.

## MCP client configuration

Use your client's standard **stdio** MCP configuration, replacing the binary path
with its actual absolute path:

```json
{
  "mcpServers": {
    "affine-desktop-local": {
      "command": "/absolute/path/affine-local-mcp",
      "args": [],
      "env": {
        "AFFINE_ENABLE_WRITES": "0"
      }
    }
  }
}
```

On Windows, use an escaped path such as
`"C:\\tools\\affine-local-mcp.exe"`. No shell or Node.js runtime is needed.

Optional environment settings:

| Setting | Meaning |
| --- | --- |
| `AFFINE_DATA_DIR` | Override desktop application data directory |
| `AFFINE_WORKSPACE_IDS` | Comma-separated allowlist of workspace IDs; unset exposes all discovered local workspaces; an empty value exposes none |
| `AFFINE_ENABLE_WRITES=1` | Enable offline creation/canvas editing; still requires AFFiNE to be closed |

Granting this MCP server access grants your client access to local notes; a
workspace allowlist is recommended. This is not a sandbox against the local OS
user who owns the files. Treat retrieved document text as untrusted data, never
as instructions.

## Tools

| Tool | Parameters / behavior |
| --- | --- |
| `affine_list_workspaces` | Lists local workspace IDs, names and active document counts |
| `affine_list_documents` | `workspaceId`, optional `offset`/`limit`; active documents and primary page/edgeless mode |
| `affine_read_document` | `workspaceId`, `documentId`, optional `offset`/`maxChars`; best-effort Markdown, warnings and continuation |
| `affine_search_documents` | `workspaceId`, `query`, optional `offset`/`limit`; literal case-insensitive title/body search |
| `affine_create_document` | `workspaceId`, `title`, `content`, optional `mode` (`page` or `edgeless`) and `documentId`; offline-only creation |
| `affine_create_diagram` | `workspaceId`, `title`, `nodes`, optional `edges`, `documentId`, `direction` (`TB` or `LR`); native shapes/connectors, not a note |
| `affine_read_canvas` | `workspaceId`, `documentId`; native primitives in `elements`, native frame/link/media blocks in `blocks`, with IDs/properties |
| `affine_edit_canvas` | `workspaceId`, `documentId`, optional `nodes`, `edges`, `strokes`, `curves`, `frames`, `links`, `media`, `deleteIds`, `confirmDelete`; atomic native-object upsert/delete |
| `affine_draw_pen` | `workspaceId`, `documentId`, `strokes`; create/replace editable native freehand strokes |
| `affine_erase_canvas` | `workspaceId`, `documentId`, `strokeIds` and/or swept `path`, optional `radius`, required `confirm: true`; whole-stroke erasing |
| `affine_import_media` | `workspaceId`, `documentId`, `media`; import local raster images or file attachments into the canvas and native blob store |

Offsets for reading are **Unicode scalar characters**, not bytes or UTF-16 units.
List/search pagination uses zero-based result offsets. Follow `hasMore` and
`nextOffset`. Search reports `partial` and warnings if a document cannot be read;
its `total` counts matches among successfully read documents.

Text extraction includes paragraphs, lists, code, simple tables, note text and
canvas text. It is not a lossless export: inline formatting may be flattened,
images/attachments have labels only, database properties are incomplete, embeds
are references, and canvas layout is not reproduced. No OCR, semantic search or
external link fetching is performed.

### Create a page

Quit AFFiNE, enable writes in your MCP client's configuration, and call:

```json
{
  "workspaceId": "<id-from-affine_list_workspaces>",
  "documentId": "my-stable-mcp-page-id",
  "title": "Created through MCP",
  "content": "This text was written by the MCP creation tool.",
  "mode": "page"
}
```

### Create an Edgeless canvas

Call the same tool with a new ID and `"mode": "edgeless"`. Nonempty content creates
an editable note positioned at `[100,100,800,300]`; empty content creates a blank
canvas without a note. `primaryMode` is persisted in
AFFiNE's `db$docProperties` document, so it opens in Edgeless mode. Content is
**plain text inside a rich-text paragraph**, not parsed Markdown.

Use a stable `documentId` for retries. Repeating the same ID/title/content/mode
returns `alreadyExists: true`; a conflicting document is rejected, never
replaced. Omitting the ID generates a new UUID on each successful invocation.

### Create a real diagram

Use **`affine_create_diagram`**, not `affine_create_document` with ASCII arrows,
Markdown or Mermaid text. Those are plain text and are not converted to shapes.
The complete example is [examples/native-flowchart.json](examples/native-flowchart.json).
Replace its `workspaceId` placeholder with a discovered local workspace ID and
send that JSON as the tool's arguments while AFFiNE is closed.

```json
{
  "workspaceId": "<local-workspace-id>",
  "documentId": "my-native-diagram",
  "title": "Native flowchart",
  "direction": "TB",
  "nodes": [
    {"id": "start", "text": "Start", "shape": "ellipse"},
    {"id": "check", "text": "Valid?", "shape": "diamond"},
    {"id": "save", "text": "Save changes", "shape": "roundedRect"}
  ],
  "edges": [
    {"id": "e1", "source": "start", "target": "check"},
    {"id": "e2", "source": "check", "target": "save", "text": "Yes"}
  ]
}
```

- Shapes: `rect`, `roundedRect`, `ellipse`, `diamond`, `triangle`.
- Each node has an editable label and optional `position: [x,y]`,
  `size: [width,height]`, `style: "general" | "scribbled"`, `fontSize`,
  `fillColor`, `strokeColor`, and `textColor`.
- Colors are hex strings or `{ "light": "#ffffff", "dark": "#222222" }` objects.
- DAGs use layered `TB` or `LR` layout. Custom/cyclic diagrams must explicitly
  position every node; use newlines and larger sizes for long labels.
- Connectors use `routing: "straight" | "orthogonal" | "curve"`, optional labels,
  `sourceAnchor`/`targetAnchor` (`top`, `right`, `bottom`, `left`) and
  `startEndpoint`/`endEndpoint` (`none`, `arrow`, `circle`, `diamond`, `triangle`).
  The default is an orthogonal line with a target arrow. Endpoints reference
  native shape IDs, so AFFiNE can re-route them when shapes move.
- Limits: 200 nodes, 500 edges, 2000 UTF-8 bytes per label, 100000 bytes of labels
  per call. Element IDs must be unique across nodes and edges.
- A stable document ID can be retried against the same persisted diagram.
  Modified diagrams are rejected rather than overwritten; use the edit tool.

### Edit an existing canvas

Inspect IDs and native properties with `affine_read_canvas`, then send nodes or
edges with the same IDs to `affine_edit_canvas`. New nodes require explicit
positions. Existing nodes retain omitted position/size, but label/appearance
defaults are applied; this is a **complete supported-element upsert**, not an
arbitrary property patch. Existing Y.Map/Y.Text identities and unrelated
elements/note blocks are preserved.

Deleting supported primitive/frame/link/media IDs requires `deleteIds` and `confirmDelete: true`.
Incident connectors are also removed to avoid dangling bindings. Repeating a
delete is safe. Native self-locked elements and locked frame ancestors are
protected. Editing grouped canvases is deliberately unsupported; ungroup in
AFFiNE first. Groups, mindmaps and note/page/surface deletion are not supported.
Removing a frame **preserves its contents**; removing a media block **does not
delete its shared binary blob**. Remaining frame references are cleaned up.

### Pen and eraser

`affine_draw_pen` takes `strokes`, each with a stable `id`, absolute `points`
(`[[x,y], ...]` or `[[x,y,pressure], ...]`), optional `lineWidth` (0.5..64,
default 4), and optional hex/theme `color`. Pressure must be 0..1 and is retained
in the native brush data; AFFiNE's renderer determines its visual use. A stroke
can be a single dot. These are native `brush` elements, **not raster images**
or shapes styled as scribbled. Limits: 5000 points/stroke, 50000 points/call.

`affine_erase_canvas` accepts explicit `strokeIds` or a swept absolute `path`
with optional `radius` (0..1000, default 8 canvas units). `confirm: true` is
mandatory. Sweeps account for brush position, rotation and line width, skip
locked strokes and return `skippedLockedIds`. Explicitly targeting a locked
stroke fails atomically. The eraser removes **whole intersecting strokes**,
not pixels or partial stroke segments, matching AFFiNE's object-erasing model.
Use a shorter path or explicit IDs if a sweep exceeds the geometry work limit.

### Frames, links and curves

Use the corresponding arrays in `affine_edit_canvas`:

- `frames`: `id`, `title`, `bounds: [x,y,width,height]`, optional `childIds`,
  `background` and `presentationIndex`. Titles remain editable Y.Text. Omit
  `childIds` to retain existing membership; `[]` explicitly empties it. Children
  must exist on the canvas. Nested frames are supported; cycles, self-membership
  and multiple frame owners are rejected. To move children between frames,
  explicitly update both frames' memberships in the same call. Frame bounds
  edits change the boundary, not the children's coordinates.
- `links`: `id`, `position`, optional `size`, `title` and `description`, plus
  `target: {"kind":"external","url":"https://..."}` or
  `target: {"kind":"document","documentId":"..."}`. External links become
  native bookmarks; document links become native same-workspace linked-doc
  cards. Targets must be active registered documents. External URLs must use
  HTTP(S) and may not contain credentials. The bridge does not fetch previews;
  the desktop app may fetch them when opened.
- `curves`: `id`, absolute `source: [x,y]` and `target: [x,y]`, optional `label`,
  `startArrow`, `endArrow`, `stroke`, `strokeWidth` and `labelColor`. These are
  **free-floating native curved connectors**, with editable labels and endpoints;
  AFFiNE computes their curve. They are not arbitrary cubic Bézier paths. For
  curves attached to shapes, use `edges` with `routing: "curve"` instead.

### Import media

The bridge only reads files deliberately placed in
**`<AFFINE_DATA_DIR>/mcp-imports`**, never arbitrary filesystem paths or URLs.
Create that staging folder and copy the files you want exposed into it. The
folder and file paths must remain inside the data directory/import root;
escaping symlinks and traversal are rejected. Once imported, the bytes are
stored in AFFiNE's native SQLite `blobs` table, so the original staging file
is no longer required to display/download the stored media.

Call `affine_import_media` with `media` items containing `id`, `filePath`,
`position`, optional `size: [width,height]`, `caption` and
`kind: "auto" | "image" | "attachment"` (default `auto`). Paths can be
import-folder-relative or absolute paths inside that folder.

- PNG/JPEG/GIF/WebP files become native image blocks. Natural dimensions are
  retained; default display geometry fits within 640x480 without upscaling.
- PDFs, video, audio and other files become native downloadable attachment
  cards. **Dedicated audio/video players are not implemented.** Other image
  formats such as SVG can be attached as files, but not imported as image blocks.
- Limits: nonempty files, 20 MiB/file, 20 files and 100 MiB/batch; images must
  have supported headers/dimensions. Canvas display size may be overridden.
- Identical file bytes reuse content-addressed blob IDs. Repeating the same
  media block ID updates it without duplicating the block or blob. Existing
  IDs of another block/element type are rejected, not silently converted.
- Blob bytes, block references, document clocks and registry changes share one
  backed-up transaction. Blob bytes/MIME/size and the complete saved block map
  are read back before commit. Deleting a block never deletes a shared blob;
  orphan-blob cleanup is left to AFFiNE rather than risking other documents.

The combined example [examples/native-canvas-features.json](examples/native-canvas-features.json)
adds a pen stroke, curve, link, image, attachment and enclosing frame atomically.
Replace its workspace/document placeholders, create a blank Edgeless document
with `affine_create_document` if needed, copy `tests/fixtures/sample-media.png`
and `tests/fixtures/sample-attachment.txt` into the staging folder, then send
the example to `affine_edit_canvas` with AFFiNE closed.

## Write safety and backups

Before each write the bridge makes a consistent SQLite online backup, including
WAL content, under:

```text
<application-data-directory>/mcp-backups/<workspace-id>-<uuid>.db
```

Backup paths are returned by the tool. Backups contain private workspace data;
retain them securely. They are not automatically deleted, including backups from
failed or repeated create attempts.

An immediate SQLite transaction atomically appends:

1. The new BlockSuite/Yjs page, surface, note and paragraph.
2. A Yjs update to the active `meta.pages` registry.
3. A Yjs update setting `primaryMode` in `db$docProperties`.
4. Matching document clock updates.

The document is reconstructed and checked before commit. Failure rolls back all
dependent updates and clocks. Reads reconstruct snapshots plus unmerged updates
from one SQLite transaction. The implementation never runs AFFiNE migrations or
changes the storage schema.

Canvas edits atomically append native primitive/block and registry `updatedDate`
updates with their clocks and imported blob rows, and reconstruct the complete
stored canvas before commit. A failed validation or dependent write leaves the
database unchanged (a backup may remain). Native shape/connector layouts follow the
[shape model](https://github.com/toeverything/AFFiNE/blob/v0.27.4/blocksuite/affine/model/src/elements/shape/shape.ts)
and [connector model](https://github.com/toeverything/AFFiNE/blob/v0.27.4/blocksuite/affine/model/src/elements/connector/connector.ts).

The verified upstream contract is
[AFFiNE v0.27.4 nbstore `push_update`](https://github.com/toeverything/AFFiNE/blob/v0.27.4/packages/frontend/native/nbstore/src/doc.rs),
[AFFiNE document registry](https://github.com/toeverything/AFFiNE/blob/v0.27.4/packages/frontend/core/src/modules/doc/stores/docs.ts),
and [desktop mode properties](https://github.com/toeverything/AFFiNE/blob/v0.27.4/packages/frontend/core/src/modules/doc/stores/doc-properties.ts).

To restore a backup, quit AFFiNE and stop the MCP server first. Preserve the
current database **and its WAL/SHM files** before any replacement. Do not restore
by copying a database over a running application.

## Verification

`cargo test`'s write integration test uses the production running-app guard;
quit AFFiNE Desktop completely before running the suite locally. The test only
writes to an isolated temporary workspace, never your real notes.

`cargo test` includes independently generated **JavaScript Yjs** fixtures (not
only Rust-to-Rust roundtrips) and a real child-process MCP stdio journey. Tests
cover creation in both modes, readback, search, pagination, retries/conflicts,
concurrent stable-ID creation, rollback after an injected dependent-write
failure, read-only defaults, running-app refusal, deleted/orphan exclusion,
corrupt/incomplete updates, allowlisting and path containment.
Native-canvas tests cover editable Y.Text labels, native element types/bindings,
all supported shape/routing variants, DAG layout, explicit cycles, editing,
confirmed/cascading deletion, preservation of unrelated notes/shapes, invalid
endpoints, read-only denial, and rollback after a dependent canvas-write failure.
Drawing/media tests cover independent JavaScript brush/frame/link/media fixtures,
pressure/relative geometry, swept erasure including rotations and dots, inherited
frame locks, retained frame titles/membership, cycle/duplicate-owner rejection,
external/local link validation, actual media-byte persistence, deduplication,
shared-blob preservation, import-root/symlink containment, and combined
canvas/blob rollback through actual MCP stdio calls.
These are storage/MCP tests, not a desktop screenshot or visual-rendering test.

The GitHub Actions matrix runs tests, formatting, Clippy and release builds on
macOS, Windows and Linux. Adding this workflow does not mean it has run; local
execution results must be reported separately.
