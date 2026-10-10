use affine_local_mcp::{server::AffineServer, storage::Storage};
use anyhow::{Context, Result, bail};
use rmcp::{ServiceExt, transport::stdio};
use std::{env, path::PathBuf};

#[tokio::main]
async fn main() -> Result<()> {
    let mut storage = Storage::from_env()?;
    let mut check = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "affine-local-mcp {}\n\nLocal stdio MCP bridge for AFFiNE Desktop.\n\nOptions:\n  --data-dir PATH    Override AFFiNE application data directory\n  --enable-writes    Enable offline writes (AFFiNE must be closed)\n  --check            Print detected workspaces and exit\n  --version         Print version\n\nEnvironment:\n  AFFINE_DATA_DIR\n  AFFINE_ENABLE_WRITES=1\n  AFFINE_WORKSPACE_IDS=id1,id2\n\nRead-only by default. No network listener or cloud credentials.",
                    env!("CARGO_PKG_VERSION")
                );
                return Ok(());
            }
            "--version" => {
                println!("{}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--data-dir" => {
                storage.data_dir = PathBuf::from(args.next().context("--data-dir requires a path")?)
            }
            "--enable-writes" => storage.writes = true,
            "--check" => check = true,
            _ => bail!("Unknown argument: {arg}; use --help"),
        }
    }
    if check {
        println!(
            "{}",
            serde_json::to_string_pretty(&storage.list_workspaces()?)?
        );
        return Ok(());
    }
    let service = AffineServer { storage }.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
