# AFFiNE Desktop MCP

A **Rust stdio MCP server for AFFiNE Desktop offline/local-only workspaces**.
It reads the desktop application's local SQLite/Yjs storage, creates normal
pages and Edgeless canvases, and creates/edits **native editable diagrams** with
shapes, rich-text labels, bound connectors, pen strokes, frames, link cards,
free-floating curves, images and file attachments.

**No AFFiNE Cloud, Docker/self-hosted AFFiNE server, GraphQL API, cloud login,
or network listener is used.** Docker is used only as an optional Linux test
runner during development, not by the application.

Connect your AI assistant to your **local AFFiNE notes and editable canvases**.
Download a standalone binary, add it to your MCP client, and ask the assistant to
list, search or read your workspaces. Creating pages and diagrams is an explicit,
offline-only opt-in.

> The Cargo, npm and Python packages are all **`affine-desktop-mcp`**.
> The npm and Python launcher command is also **`affine-desktop-mcp`**.
> The standalone executable (including Cargo installs) remains **`affine-local-mcp`**
> (`affine-local-mcp.exe` on Windows). Registry packages are prepared here but
> have **not yet been published**; registry install commands below apply after release.

## Contents

- [Quick start](#quick-start)
- [Install a release](#install-a-release)
- [Install with npm or pip](#install-with-npm-or-pip)
- [Build from source](#build-from-source)
- [Desktop data directories](#desktop-data-directories)
- [Configuration options](#configuration-options)
- [Connect your MCP client](#connect-your-mcp-client)
- [First conversation](#first-conversation)
- [Tools and examples](#tools)
- [Write safety and backups](#write-safety-and-backups)
- [Troubleshooting](#troubleshooting)
- [Development and releases](#releases)

## Quick start

1. Install AFFiNE Desktop and create/open a **local-only workspace**. This bridge
   does not expose AFFiNE Cloud workspaces. The tested storage version is **0.27.4**.
2. [Download and extract](#install-a-release) the binary for your OS and CPU.
   Prebuilt binaries need no Rust, Node.js, Python, Docker or cloud API key.
3. Run the binary with `--version`, then `--check` to discover workspace IDs:

   ```sh
   /absolute/path/affine-local-mcp --version
   /absolute/path/affine-local-mcp --check
   ```

   Windows PowerShell:

   ```powershell
   & 'C:\tools\affine-local-mcp.exe' --version
   & 'C:\tools\affine-local-mcp.exe' --check
   ```

4. [Configure your client](#connect-your-mcp-client) with the binary's **absolute
   path**. Keep writes disabled initially. Add a workspace allowlist after
   discovering the IDs you want to expose.
5. Restart/reconnect the client's MCP server and ask:
   **“Use the AFFiNE tools to list my local workspaces, then list their documents.”**

The client launches the server for you. Do **not** launch a separate background
server, enter a URL, or include `--check` in its MCP launch arguments.

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

## Install a release

Open the [latest release](https://github.com/sauravniraula/affine-desktop-mcp/releases/latest)
or the [v0.1.0 release](https://github.com/sauravniraula/affine-desktop-mcp/releases/tag/v0.1.0).
Download **one** archive matching the machine where the MCP client runs, plus
`SHA256SUMS` if you want to verify its integrity.

| Machine | v0.1.0 archive |
| --- | --- |
| macOS, Apple Silicon (M1/M2/M3/M4 and other arm64 Macs) | `affine-local-mcp-0.1.0-macos-arm64.tar.gz` |
| macOS, Intel | `affine-local-mcp-0.1.0-macos-amd64.tar.gz` |
| Linux, x86_64 / amd64 | `affine-local-mcp-0.1.0-linux-amd64.tar.gz` |
| Linux, aarch64 / arm64 | `affine-local-mcp-0.1.0-linux-arm64.tar.gz` |
| Windows, Intel/AMD x64 | `affine-local-mcp-0.1.0-windows-amd64.zip` |
| Windows, ARM64 | `affine-local-mcp-0.1.0-windows-arm64.zip` |

Linux downloads require **glibc 2.35 or later**; they are not musl/Alpine binaries.
Each archive includes the executable, README and Apache 2.0 license.

### macOS and Linux

For example, on an **Apple Silicon Mac**, run these commands in a download folder:

```sh
curl -fLO https://github.com/sauravniraula/affine-desktop-mcp/releases/download/v0.1.0/affine-local-mcp-0.1.0-macos-arm64.tar.gz
curl -fLO https://github.com/sauravniraula/affine-desktop-mcp/releases/download/v0.1.0/SHA256SUMS
shasum -a 256 affine-local-mcp-0.1.0-macos-arm64.tar.gz
```

Compare the printed hash with the line for that archive in `SHA256SUMS` **before
extracting it**. On Linux you can use `sha256sum` instead of `shasum -a 256`.
For another OS/CPU, substitute the filename from the table in both the URL and
the commands.

```sh
mkdir -p affine-desktop-mcp-0.1.0
tar -xzf affine-local-mcp-0.1.0-macos-arm64.tar.gz -C affine-desktop-mcp-0.1.0
mkdir -p "$HOME/.local/bin"
install -m 755 affine-desktop-mcp-0.1.0/affine-local-mcp "$HOME/.local/bin/affine-local-mcp"
"$HOME/.local/bin/affine-local-mcp" --version
"$HOME/.local/bin/affine-local-mcp" --check
```

Use the expanded absolute path (for example,
`/Users/alice/.local/bin/affine-local-mcp`) in client configuration. Adding
`~/.local/bin` to `PATH` is optional when you use an absolute path.
If macOS blocks the executable, review its source and checksum, then use the
system's **Privacy & Security** approval flow; do not disable Gatekeeper globally.

### Windows PowerShell

For example, on an **x64 Windows PC**, run in a download folder:

```powershell
Invoke-WebRequest 'https://github.com/sauravniraula/affine-desktop-mcp/releases/download/v0.1.0/affine-local-mcp-0.1.0-windows-amd64.zip' -OutFile 'affine-local-mcp-0.1.0-windows-amd64.zip'
Invoke-WebRequest 'https://github.com/sauravniraula/affine-desktop-mcp/releases/download/v0.1.0/SHA256SUMS' -OutFile 'SHA256SUMS'
Get-FileHash '.\affine-local-mcp-0.1.0-windows-amd64.zip' -Algorithm SHA256
Get-Content '.\SHA256SUMS'
```

Compare the hash against the matching filename (hash letter case is irrelevant),
then extract the archive to a permanent directory you own:

```powershell
Expand-Archive '.\affine-local-mcp-0.1.0-windows-amd64.zip' -DestinationPath "$env:USERPROFILE\tools\affine-desktop-mcp-0.1.0"
& "$env:USERPROFILE\tools\affine-desktop-mcp-0.1.0\affine-local-mcp.exe" --version
& "$env:USERPROFILE\tools\affine-desktop-mcp-0.1.0\affine-local-mcp.exe" --check
```

For Windows ARM64, substitute `windows-arm64` in the download and extraction
commands. In JSON, use an expanded path such as
`"C:\\Users\\alice\\tools\\affine-desktop-mcp-0.1.0\\affine-local-mcp.exe"`.

### Upgrading

Stop the server in your MCP client before replacing its executable, verify the
new archive, then reconnect. Keep your data-directory and workspace-allowlist
settings. Upgrading the bridge does not migrate AFFiNE's database. Review release
notes and AFFiNE version compatibility before enabling writes again.

## Install with npm or pip

**These packages have not yet been published.** The following registry commands
will work after publication. Both packages bundle the same native Rust server;
they do not reimplement it or enable writes. Installation uses the package
registry, but the installed server runs offline and never downloads a binary at
startup. macOS, glibc Linux and Windows are supported on x64 and arm64.

### npm / npx

Requires Node.js **18 or later**. npm automatically installs the matching native
optional dependency; do not use `--omit=optional`.

```sh
npm install -g affine-desktop-mcp
affine-desktop-mcp --version
affine-desktop-mcp --check
```

Or run without a global installation:

```sh
npx --yes affine-desktop-mcp --check
```

For a stdio MCP client, use `"command": "npx"` and
`"args": ["--yes", "affine-desktop-mcp"]`. On Windows use `npx.cmd` if your
client requires the command shim. For reproducibility, pin the package in args
to a published version, for example `affine-desktop-mcp@0.2.2`. GUI clients may
need the absolute path to `npx` or the installed launcher.

### pip / pipx / uvx

Requires Python **3.11 or later**. Use a virtual environment with pip, or use
pipx/uvx for an isolated CLI installation. Supported-platform wheels require no
Rust compiler; a source installation needs the [Rust/C build tools](#build-from-source).
Linux release wheels require glibc 2.35 or later (not Alpine/musl).

```sh
python -m venv .venv
# macOS/Linux: source .venv/bin/activate
# Windows PowerShell: .venv\Scripts\Activate.ps1
python -m pip install affine-desktop-mcp
affine-desktop-mcp --version
affine-desktop-mcp --check
```

Alternatively:

```sh
pipx install affine-desktop-mcp
# Or run without a persistent installation:
uvx affine-desktop-mcp --check
```

Use the installed launcher's absolute path as your MCP client's `command`, with
empty `args`, or use `"command": "uvx"`, `"args": ["affine-desktop-mcp"]`.
Pin a published version with `affine-desktop-mcp==0.2.2` in uvx's args.
`python -m affine_desktop_mcp` is also supported. Do not put diagnostic flags
such as `--check` into the client's launch arguments. The configuration variables,
allowlists and offline-write safety rules below apply to both launchers unchanged.

### Local packages before publication

From a checkout, build a source installation with
`python -m pip install .` in a virtual environment (Rust and a C compiler required).
To build installable release artifacts for the current machine:

```sh
python -m pip install build
cargo build --release --locked
# Apple Silicon example; use linux/windows and amd64/arm64 for your machine.
# On Windows the binary ends in .exe.
python .github/scripts/package_registries.py --binary target/release/affine-local-mcp --platform macos --arch arm64
python .github/scripts/package_registries.py --shared
python .github/scripts/smoke_packages.py --platform macos --arch arm64
```

Artifacts are written to `dist/`. The smoke test installs the main npm tarball
**and** its matching native tarball together, installs the wheel in a fresh
virtual environment, and exercises actual MCP stdio calls against an isolated
empty data directory. It does not read or write your real notes. Installing just
the wheel needs no compiler; add `--source` to the smoke command to also verify a
source-archive installation using Rust. Installing just
the npm wrapper from this unpublished checkout cannot fetch unpublished native
dependencies. Local Linux wheels have a `linux_*` tag; the release workflow audits
and repairs them to `manylinux_*` before they can be uploaded to PyPI.

## Build from source

Install Rust **1.95 or later** and a C compiler (SQLite is bundled):

- macOS: Xcode Command Line Tools.
- Windows: Rust's MSVC toolchain and Visual Studio Build Tools with Desktop
  development with C++.
- Linux: a C toolchain such as `build-essential` on Debian/Ubuntu.

```sh
git clone https://github.com/sauravniraula/affine-desktop-mcp.git
cd affine-desktop-mcp
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
`cargo check`, and set the same version in `package.json` and all six of its
`optionalDependencies`. Python metadata derives its version from Cargo (Python
prerelease spelling is normalized by setuptools). Commit the changes, then push
a matching **new** tag such as `v0.2.0`; existing releases are not replaced.
Only pushes of tags matching `v*` trigger the release workflow; unprefixed tags
such as `0.1.0` do not. The version after `v` must match the manifest exactly,
including any prerelease/build suffix.

Before any build, the workflow skips publication if the tag does not match
the manifest or a release already exists for that version (with or without
the `v` prefix, including drafts and prereleases). GitHub API errors fail the
check rather than being treated as permission to release. Publication runs
only after all six builds and tests succeed, and checks for an existing release
again before uploading. Prerelease versions are marked as GitHub prereleases.

The workflow also builds/tests six native npm tarballs and six platform-specific
Python wheels, plus the main npm tarball and Python source distribution. Linux
wheels are checked/repaired with auditwheel for manylinux/glibc compatibility.
These artifacts and `SHA256SUMS` are attached to the GitHub release; **the workflow
then automatically publishes to crates.io, npm and PyPI** using GitHub Actions
secrets. A dry-run-verified Cargo `.crate` archive is also attached to the release.
Registry jobs download these exact assets and verify `SHA256SUMS` before uploading.

### Required GitHub Actions secrets

In this repository, open **Settings → Secrets and variables → Actions → New
repository secret** and add these exact names. The workflow checks that all three
exist before starting a new release build. Never paste tokens into issues, chat,
workflow YAML, or committed configuration files.

| Secret | Value and permissions |
| --- | --- |
| `CARGO_REGISTRY_TOKEN` | A [crates.io API token](https://crates.io/settings/tokens) scoped to **`affine-desktop-mcp`**, with **Publish new crates** (`publish-new`) for the first release and **Publish updates to existing crates** (`publish-update`) for later versions. No owner-management or yank permissions are needed. Verify the account's email address and ensure you own/can create the crate name. |
| `NPM_TOKEN` | An [npm granular access token](https://docs.npmjs.com/creating-and-viewing-access-tokens) with **Read and write (publish and stage)** permissions and **Bypass two-factor authentication** enabled for unattended publishing. For the initial creation of the seven unscoped packages, use a token allowed to create them (typically **All Packages**); afterward restrict it to the seven package names listed below. Do not use a read-only, stage-only or legacy token. Package settings must permit token publishing. |
| `PYPI_API_TOKEN` | A production [PyPI API token](https://pypi.org/manage/account/token/), including its `pypi-` prefix. For the first release, use an account-wide token so it can create **`affine-desktop-mcp`**. After creation, replace it with a token scoped only to that project. Verify your email and complete PyPI's account/2FA requirements. A TestPyPI token will not work. The workflow supplies username `__token__` automatically. |

The npm token must cover all of:

- `affine-desktop-mcp`
- `affine-desktop-mcp-darwin-x64`
- `affine-desktop-mcp-darwin-arm64`
- `affine-desktop-mcp-linux-x64`
- `affine-desktop-mcp-linux-arm64`
- `affine-desktop-mcp-win32-x64`
- `affine-desktop-mcp-win32-arm64`

All package names must be available to or owned by your registry accounts.
Creating local manifests does not reserve names. Choose short-lived tokens and
rotate/update the corresponding GitHub secret before expiration. Standard GitHub
hosted runners do not have fixed outbound IPs; do not apply an incompatible token
IP allowlist.

**No custom GitHub PAT, SSH key, signing key, or registry username secret is
required.** GitHub supplies `GITHUB_TOKEN` automatically; the GitHub-release job
declares `contents: write`, while registry jobs use read-only GitHub permissions.
Allow Actions to run in repository settings and ensure organization policies do
not block that permission. The standalone binaries remain unsigned; Apple
notarization and Windows Authenticode signing are not part of this workflow.

### Publication and recovery

After committing the packaging/workflow changes, set a new matching version in
Cargo and npm metadata and push its `v...` tag. Existing GitHub releases are not
overwritten or automatically backfilled. Monitor **Actions → Release**:

1. Validate the tag/manifests and required secrets.
2. Build/test all six native targets, install/test their packages and audit Linux wheels.
3. Dry-run the Cargo publication, create the GitHub release and verify its assets.
4. Independently publish `affine-desktop-mcp` to crates.io, all seven npm packages,
   and all six Python wheels plus the source distribution to PyPI.

Stable npm versions use `latest`; prereleases use `next` on all seven packages.
Python prerelease versions use normalized PEP 440 spelling. Registry jobs read
back public registry metadata and compare uploaded checksums. Existing identical
files are safely skipped; conflicting or yanked files fail rather than being
overwritten. Native npm dependencies are published and verified before the main
launcher. Tokens are passed only to secret preflight and the corresponding
publication step, not to build/test jobs.

Publication across independent registries is **not atomic**: the GitHub release
or some registry packages may remain if another registry fails. Fix the cause
(for example an expired token) and select **Re-run failed jobs**, not all jobs.
Alternatively, use **Actions → Release → Run workflow**, select the default branch
containing this workflow, and supply the existing release tag in the `tag` field.
This recovery mode checks out that exact tag, downloads its original release
assets, verifies checksums and resumes registry publication **without rebuilding
binaries or replacing the GitHub release**. It requires an existing published
release produced by this workflow, not an old binary-only release or a draft.

Once publication succeeds, check fresh registry installs with `cargo install
affine-desktop-mcp --locked`, `npx --yes affine-desktop-mcp --version` and `uvx
affine-desktop-mcp --version`. No registry credentials have been added to this
repository, and editing this workflow alone does not publish anything.

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

For example, if your database is at
`/home/alice/.config/AFFiNE/workspaces/local/abc123/storage.db`, set
`AFFINE_DATA_DIR` to `/home/alice/.config/AFFiNE` and use `abc123` as the workspace ID.
This same rule applies to custom locations on macOS and Windows.

## Configuration options

The bridge has no separate config file. Configure environment variables and/or
launch arguments in your MCP client. Use **literal, expanded absolute paths**;
the binary does not expand `~`, `$HOME` or `%APPDATA%` in supplied paths.
Your shell may expand them when you run a diagnostic command, but JSON clients
have different interpolation rules.

### Environment variables

| Variable | Default | Behavior |
| --- | --- | --- |
| `AFFINE_DATA_DIR` | [OS-specific directory](#desktop-data-directories) | Directory **containing `workspaces`**, not `storage.db` or the workspace directory itself. |
| `AFFINE_WORKSPACE_IDS` | Unset: all discovered local workspaces | Comma-separated allowlist of workspace IDs. Whitespace is trimmed. An explicitly empty string exposes **no** workspaces. IDs must contain 1–128 ASCII letters, digits, underscores or hyphens. Discover actual IDs with `--check`; names/titles are not IDs. |
| `AFFINE_ENABLE_WRITES` | Disabled | Only the exact string `"1"` enables writes; `"0"`, `"true"` and other values do not. AFFiNE must still be completely closed. |

Default discovery also uses `APPDATA` on Windows and `XDG_CONFIG_HOME` on Linux.
If your client filters environment variables, explicitly set `AFFINE_DATA_DIR`
instead of relying on inherited shell settings.

### Command-line options

| Option | Behavior |
| --- | --- |
| `--data-dir PATH` | Overrides `AFFINE_DATA_DIR` / automatic discovery for this process. Pass the flag and path as **separate** elements in `args`. |
| `--enable-writes` | Enables writes, even when `AFFINE_ENABLE_WRITES` is `"0"`. Omit this flag for read-only operation. There is no `--disable-writes` flag. |
| `--check` | Prints detected workspaces, warnings and `writesEnabled` as JSON, then exits. Diagnostic only. |
| `--version` | Prints the package version, then exits. Diagnostic only. |
| `--help`, `-h` | Prints usage, then exits. Diagnostic only. |
| No diagnostic option | Starts the stdio MCP server and waits for requests from its client. |

For a custom data directory, the launch arguments can be:

```json
["--data-dir", "/absolute/path/to/AFFiNE"]
```

Neither the bridge nor a desktop client necessarily reads your shell profile or
an `.env` file. Set values in the client's server configuration explicitly.
There is no port, URL, API token, cloud login or HTTP/SSE mode to configure.

Here is a **restricted, custom-directory** server entry to merge under a client's
`mcpServers` object. Replace all three placeholders before using it:

```json
{
  "affine-desktop": {
    "command": "/absolute/path/affine-local-mcp",
    "args": [],
    "env": {
      "AFFINE_DATA_DIR": "/absolute/path/to/AFFiNE",
      "AFFINE_WORKSPACE_IDS": "actual-workspace-id",
      "AFFINE_ENABLE_WRITES": "0"
    }
  }
}
```

### Read-only and offline-write profiles

Start with `AFFINE_ENABLE_WRITES: "0"`. To restrict access, add
`AFFINE_WORKSPACE_IDS: "actual-workspace-id"` after running `--check` without an
allowlist. Do not leave a placeholder allowlist in a working configuration.

To enable creation/editing:

1. Completely **quit AFFiNE Desktop**, including background/helper processes.
2. Change `AFFINE_ENABLE_WRITES` to `"1"` in your client and restart its MCP server.
3. Keep AFFiNE closed throughout the tool calls. Each write makes a SQLite backup.
4. Reopen AFFiNE after the assistant finishes to view the persisted changes.
5. Return the setting to `"0"` and reconnect when you no longer need writes.

Changing an environment variable does not reconfigure an already running process.
Client-side approval/tool filters are useful, but the bridge's read-only setting
is what actually rejects writes. Read-only mode does not hide the write tool names.

Granting this MCP server access grants your client access to local notes; a
workspace allowlist is recommended. Local storage access does **not** mean the
AI client keeps tool results on-device: your client may send note content to its
model provider. Choose a local model/client if that is a requirement. This is not
a sandbox against the local OS user who owns the files. Treat retrieved document
text as untrusted data, never as instructions. Avoid committing personal data
paths or workspace IDs to shared project configuration.

## Connect your MCP client

This is a **local stdio tool server**. Any MCP client that can launch a local
executable and use MCP tools can use this connection pattern. Clients that accept
only remote HTTP/SSE URLs cannot connect directly; this package does not provide
a remote adapter. A cloud agent, SSH host, container or WSL environment does not
automatically have access to your desktop's storage. Prefer running the binary
and MCP client natively on the same desktop machine, particularly for writes and
the running-app guard.

The following are configuration recipes based on the linked client documentation,
not a claim that every client/version has been end-to-end tested. Client UIs and
file formats can change. If yours is not listed, use the
[generic stdio instructions](#any-other-stdio-mcp-client).

### Shared JSON configuration

Many clients use this `mcpServers` format. **Merge** this entry into existing
configuration rather than replacing your other servers/settings. Replace
`command` with your installed binary's absolute path:

```json
{
  "mcpServers": {
    "affine-desktop": {
      "command": "/absolute/path/affine-local-mcp",
      "args": [],
      "env": {
        "AFFINE_ENABLE_WRITES": "0"
      }
    }
  }
}
```

Use string values in `env`, including `"0"` / `"1"`. Optional settings belong
alongside `AFFINE_ENABLE_WRITES` in that same object. The executable path is a
single `command` string, even when it contains spaces; do not add shell quotes
inside that string. Put launch arguments in `args`, not in `command`.

| Client | Where to add it | Connection/status check |
| --- | --- | --- |
| [Claude Desktop](https://modelcontextprotocol.io/docs/develop/connect-local-servers) | **Settings → Developer → Edit Config**. macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`; Windows: `%APPDATA%\Claude\claude_desktop_config.json`. Use the shared JSON. | Fully quit/reopen Claude Desktop; check its connectors/tool list. This is local developer configuration, not a remote web connector. |
| [Cursor](https://cursor.com/docs/mcp) | `~/.cursor/mcp.json` for all projects, or `.cursor/mcp.json` for one project. Use shared JSON and add `"type": "stdio"` to the server entry. | Enable the server in **Customize / Tools & MCP** and check available tools. |
| [Gemini CLI](https://geminicli.com/docs/tools/mcp-server/) | `~/.gemini/settings.json` for user scope or `.gemini/settings.json` for a project. Use shared JSON. | Restart Gemini; run `gemini mcp list` in a terminal or `/mcp` in a session. Keep tool confirmations enabled. |
| [Cline](https://docs.cline.bot/mcp/mcp-overview) | Extension: **MCP Servers → Configure → Configure MCP Servers**. CLI: `~/.cline/data/settings/cline_mcp_settings.json`. Use shared JSON. | Restart the server and verify tools in the MCP panel. Leave `autoApprove` empty/omit it until you review the tools. |
| [Roo Code](https://docs.roocode.com/features/mcp/using-mcp-in-roo) | **MCP settings → Edit Global MCP**, or `.roo/mcp.json` for a project. Use shared JSON. | Enable/restart the server in the MCP panel; do not blanket-allow mutating tools. |
| [LM Studio](https://lmstudio.ai/docs/app/plugins/mcp) | **Program → Install → Edit mcp.json**. Merge the shared JSON server entry. | Enable the server for the conversation and use a model that supports tool calling. |
| [Windsurf / Cascade](https://docs.windsurf.com/windsurf/cascade/mcp) | Open the MCP config from the agent's MCP settings and merge shared JSON. Legacy Windsurf uses `~/.codeium/windsurf/mcp_config.json`. Newer Devin Desktop/Cascade docs use `~/.config/devin/mcp_config.json` (Windows: `%APPDATA%\devin\mcp_config.json`). | Use the file opened by **your installed version's UI**, not an assumed legacy path; enable/restart the server. Devin Local uses the [Devin CLI configuration](https://docs.devin.ai/cli/extensibility/mcp/configuration). |

The clients below have different configuration shapes; do not paste `mcpServers`
unchanged into their native configuration.

### Claude Code

Register the server for your user (all projects):

```sh
claude mcp add --transport stdio --scope user --env AFFINE_ENABLE_WRITES=0 affine-desktop -- /absolute/path/affine-local-mcp
claude mcp list
```

Use `/mcp` inside Claude Code to inspect the connection. To keep it project-scoped,
use `--scope local`; to deliberately share configuration, use `--scope project`.
Add other variables with additional `--env NAME=value` options before `--`.
Server arguments go after the binary, for example
`-- /absolute/path/affine-local-mcp --data-dir /absolute/path/to/AFFiNE`.
See [Claude Code MCP documentation](https://code.claude.com/docs/en/mcp).

### Codex CLI, IDE extension and desktop clients

Add this TOML to `~/.codex/config.toml`, or `.codex/config.toml` in a trusted project:

```toml
[mcp_servers.affine-desktop]
command = "/absolute/path/affine-local-mcp"
args = []

[mcp_servers.affine-desktop.env]
AFFINE_ENABLE_WRITES = "0"
```

For a Windows path, TOML literal strings avoid backslash escaping:
`command = 'C:\tools\affine-local-mcp.exe'`.
Reconnect/restart the client and inspect its MCP server list. Codex clients on
the same host share this configuration; a hosted web-only connector is not a
local stdio process. See [Codex MCP documentation](https://developers.openai.com/codex/mcp).

### VS Code / GitHub Copilot

For current versions, create a project-root `.mcp.json` using the shared JSON,
or use **MCP: Add Server → Command (stdio)** and choose a local/user destination.
The portable user configuration is `~/.copilot/mcp-config.json` (or under
`COPILOT_HOME` when set).

For VS Code's `.vscode/mcp.json` format, the top-level key is **`servers`**:

```json
{
  "servers": {
    "affine-desktop": {
      "type": "stdio",
      "command": "/absolute/path/affine-local-mcp",
      "args": [],
      "env": { "AFFINE_ENABLE_WRITES": "0" }
    }
  }
}
```

Use **MCP: List Servers** to start the server/view logs, then enable its tools in
an agent chat. With SSH/Dev Containers, choose configuration that runs **locally
on the desktop host**, not in a remote environment without AFFiNE's files.
See [VS Code MCP documentation](https://code.visualstudio.com/docs/agent-customization/mcp-servers).

### Zed

Use **Settings → AI → MCP Servers → Add Server → Add Local Server**, or merge this
into the settings file opened by `zed: open settings file`:

```json
{
  "context_servers": {
    "affine-desktop": {
      "command": "/absolute/path/affine-local-mcp",
      "args": [],
      "env": { "AFFINE_ENABLE_WRITES": "0" }
    }
  }
}
```

Check the server status in MCP settings and use its tools in Zed Agent. External
agents/terminal threads may read their own native client configuration instead.
See [Zed MCP documentation](https://zed.dev/docs/ai/mcp).

### OpenCode

Merge into `opencode.json` in your project or
`~/.config/opencode/opencode.json` for user-wide access:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "affine-desktop": {
      "type": "local",
      "command": ["/absolute/path/affine-local-mcp"],
      "environment": { "AFFINE_ENABLE_WRITES": "0" },
      "enabled": true
    }
  }
}
```

Unlike the other JSON formats, **`command` is an array** containing the executable
and any launch arguments, and variables go in **`environment`**, not `env`.
Run `opencode mcp list` to check the connection.
See [OpenCode MCP documentation](https://opencode.ai/docs/mcp-servers/).

### Hermes Agent

Use Hermes' config command to add the server without hand-editing YAML:

```sh
hermes config set mcp_servers.affine-desktop '{"command":"/absolute/path/affine-local-mcp","args":[],"env":{"AFFINE_ENABLE_WRITES":"0"}}'
```

Use the active profile's configuration (`HERMES_HOME` / `hermes --profile NAME`),
not another profile's config. Restart/reconnect Hermes; tool names may be prefixed
with the server name. Put custom data-directory and allowlist settings in this
entry's `env`, since the client's inherited environment may be filtered.
The command above uses POSIX shell quoting; on Windows, use an appropriate shell
or the client's MCP configuration UI.
See [Hermes MCP documentation](https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp).

### Any other stdio MCP client

Use the client's **Add local MCP server / Command / stdio** workflow:

| Field | Value |
| --- | --- |
| Name | `affine-desktop` (a label; you may choose another) |
| Transport | `stdio` / local process |
| Command / executable | Absolute path to `affine-local-mcp` or `affine-local-mcp.exe` |
| Arguments | Empty initially; optional `--data-dir` and its path as separate arguments |
| Environment | `AFFINE_ENABLE_WRITES=0`; optionally `AFFINE_DATA_DIR` and `AFFINE_WORKSPACE_IDS` |
| URL / headers / OAuth | Not applicable; leave unset |

Save, reconnect, approve the server if prompted, and verify that
`affine_list_workspaces` appears. For standalone downloads, the extracted
executable is the command, not a GitHub URL. After registry publication, you can
also use the [npm/uvx launcher recipes](#install-with-npm-or-pip). The client must
support MCP **tool calling**, not just resources/prompts or remote connectors.

## First conversation

1. **Discover:** “Call `affine_list_workspaces` and show me the workspace IDs.”
2. **Browse:** “List the documents in workspace `<id>` using the AFFiNE tools.”
3. **Read/search:** “Search that workspace for ‘project plan’, then read the matching
   document. Follow pagination until you have the full text.”
4. **Optional write:** Quit AFFiNE, enable writes and reconnect, then ask:
   “Create a page titled ‘Meeting notes’ in workspace `<id>` with this content.
   Use document ID `meeting-notes-mcp` and report the backup path.”
5. **Native diagram:** “Use `affine_create_diagram` to create an editable Start →
   Review → Done flowchart. Do not put ASCII or Mermaid into a text note.”

Replace `<id>` with a discovered ID. Review the client's tool arguments and
approvals. Tool discovery/connection alone does not prove storage access; a
successful workspace listing and document read are the useful first checks.

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

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| Client says “command not found” / `ENOENT` | Extract the archive first and use the absolute executable path, not the archive, directory, GitHub URL or Cargo package name. GUI clients may have a different `PATH` from your shell. On Windows include `.exe` and escape JSON backslashes. |
| “Permission denied” or macOS blocks launch | Ensure the file is executable (`chmod +x /absolute/path/affine-local-mcp` on macOS/Linux). Review macOS Privacy & Security prompts and the download checksum. Do not bypass security globally. |
| “Exec format error” / wrong architecture / missing glibc | Choose the OS/CPU archive from the installation table. Linux binaries need glibc 2.35+; build from source for other supported environments. |
| Running the binary appears to hang | Without a diagnostic flag it is waiting for MCP requests on stdin. This is normal; let your client launch it. Use `--check` for a standalone diagnostic. |
| Server exits immediately or tools never appear | Remove `--check`, `--help` and `--version` from MCP `args`. Confirm stdio transport, the correct client config shape, and that the client enabled/trusted the server. Read its MCP stderr logs. |
| “No local workspaces” | Open/create a local-only workspace in AFFiNE and let it persist. Run `--check`. For a custom install, point `AFFINE_DATA_DIR` / `--data-dir` at the directory containing `workspaces`. Cloud caches are deliberately excluded. |
| Workspace missing or “Unknown or disallowed local workspace” | Check `AFFINE_WORKSPACE_IDS`. An empty value exposes none; a name instead of an ID will not match. Discover IDs without an allowlist, then reconnect with the intended IDs. |
| Listing succeeds but documents have warnings | Check persisted data, schema compatibility and AFFiNE version. Inspect `warnings` / `partial`; do not treat partial search results as a complete export. |
| “Writes disabled” | Keep read-only mode unless you need a mutation. For writes, quit AFFiNE, set `AFFINE_ENABLE_WRITES` to the string `"1"` and restart the client's server. |
| “Quit AFFiNE Desktop before writing” | Closing its window may not quit background processes. Completely quit it and keep it closed throughout the operation. Do not defeat the process guard. |
| Edits are not visible in AFFiNE | Finish all offline writes, then reopen AFFiNE. There is no live editor synchronization; readers also see only persisted content. |
| “Unsupported storage schema” / “Incomplete Yjs data” | Do not edit SQLite manually or force a migration. Let AFFiNE finish persisting, check the tested 0.27.4 contract, and report the exact error without sharing private notes/databases. |
| Media path rejected | Copy the file into `<AFFINE_DATA_DIR>/mcp-imports`. Arbitrary filesystem paths, URL imports, escaping symlinks and files above the documented limits are not accepted. |

For a custom location, diagnose with the **same path and environment** used by the
client. Example (macOS/Linux):

```sh
AFFINE_ENABLE_WRITES=0 /absolute/path/affine-local-mcp --data-dir /absolute/path/to/AFFiNE --check
```

Windows PowerShell:

```powershell
$env:AFFINE_ENABLE_WRITES = '0'
& 'C:\tools\affine-local-mcp.exe' --data-dir 'C:\Users\alice\AppData\Roaming\AFFiNE' --check
```

Restart/reconnect the client after any config change. If a command works in your
terminal but not in the client, compare executable paths, environment variables,
permissions and whether the client runs locally or remotely. Consult the client's
MCP logs, not stdout logging wrappers: stdout must contain only MCP messages.
When [reporting an issue](https://github.com/sauravniraula/affine-desktop-mcp/issues),
include OS/architecture, bridge `--version`, AFFiNE version, client/version and a
redacted error/config. Do not upload workspace databases or private tool results.

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

The [GitHub Actions matrix](https://github.com/sauravniraula/affine-desktop-mcp/actions)
runs tests, formatting, Clippy and release builds on macOS, Windows and Linux.
The release workflow additionally tests/builds all six OS/architecture targets
before publishing. Client-specific configuration recipes above are documentation,
not an end-to-end certification of every client UI or model. Release archives
contain the README from their tagged commit; this repository's default branch
contains the latest setup guidance.
