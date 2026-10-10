"""Build npm tarballs and Python distributions without publishing anything."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
PLATFORMS = {"macos": "darwin", "linux": "linux", "windows": "win32"}
ARCHITECTURES = {"amd64": "x64", "arm64": "arm64"}


def metadata(root=ROOT):
    with (root / "Cargo.toml").open("rb") as source:
        cargo = tomllib.load(source)["package"]
    if cargo["name"] != "affine-desktop-mcp":
        raise ValueError("Cargo package name must be affine-desktop-mcp")
    version = cargo["version"]
    package = json.loads((root / "package.json").read_text())
    expected = {
        f"affine-desktop-mcp-{platform}-{arch}": version
        for platform in PLATFORMS.values()
        for arch in ARCHITECTURES.values()
    }
    if package["name"] != "affine-desktop-mcp" or package["version"] != version:
        raise ValueError("npm name/version must match affine-desktop-mcp and Cargo.toml")
    if package["optionalDependencies"] != expected:
        raise ValueError("All six npm optional dependency versions must match Cargo.toml")
    with (root / "pyproject.toml").open("rb") as source:
        project = tomllib.load(source)["project"]
    if project["name"] != "affine-desktop-mcp" or "version" not in project["dynamic"]:
        raise ValueError("Python name must be affine-desktop-mcp with version derived from Cargo.toml")
    return package


def pack_npm(directory, dist):
    npm = shutil.which("npm")
    if npm is None:
        raise RuntimeError("npm is required to package npm distributions")
    subprocess.run(
        [npm, "pack", "--ignore-scripts", "--pack-destination", str(dist)],
        cwd=directory, check=True,
    )


def package_npm_binary(binary, platform, arch, root=ROOT):
    package = metadata(root)
    node_platform = PLATFORMS[platform]
    node_arch = ARCHITECTURES[arch]
    binary = Path(binary).resolve()
    executable = "affine-local-mcp.exe" if platform == "windows" else "affine-local-mcp"
    if not binary.is_file() or binary.name != executable:
        raise ValueError(f"Expected a built {executable}: {binary}")
    dist = (root / "dist").resolve()
    dist.mkdir(exist_ok=True)
    # tempfile honors TMPDIR locally, and keeps staging out of the source tree.
    with tempfile.TemporaryDirectory(prefix="affine-npm-") as temporary:
        staging = Path(temporary)
        native = {
            key: package[key]
            for key in ("version", "description", "license", "repository", "homepage", "bugs")
        }
        native.update({
            "name": f"affine-desktop-mcp-{node_platform}-{node_arch}",
            "os": [node_platform],
            "cpu": [node_arch],
            "files": ["bin/", "README.md", "LICENSE"],
        })
        if platform == "linux":
            native["libc"] = ["glibc"]
        (staging / "package.json").write_text(json.dumps(native, indent=2) + "\n")
        (staging / "bin").mkdir()
        destination = staging / "bin" / executable
        shutil.copy2(binary, destination)
        destination.chmod(0o755)
        for name in ("README.md", "LICENSE"):
            shutil.copy2(root / name, staging / name)
        pack_npm(staging, dist)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--platform", choices=PLATFORMS)
    parser.add_argument("--arch", choices=ARCHITECTURES)
    parser.add_argument("--shared", action="store_true", help="Build the main npm package and Python sdist")
    args = parser.parse_args()
    metadata()
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    if args.shared:
        if args.binary or args.platform or args.arch:
            parser.error("--shared cannot be combined with native package arguments")
        pack_npm(ROOT, dist)
        subprocess.run([sys.executable, "-m", "build", "--sdist", "--outdir", str(dist)], cwd=ROOT, check=True)
    else:
        if not all((args.binary, args.platform, args.arch)):
            parser.error("Native packages require --binary, --platform and --arch")
        package_npm_binary(args.binary, args.platform, args.arch)
        env = os.environ.copy()
        env["AFFINE_DESKTOP_MCP_BINARY"] = str(args.binary.resolve())
        subprocess.run([sys.executable, "-m", "build", "--wheel", "--outdir", str(dist)], cwd=ROOT, env=env, check=True)


if __name__ == "__main__":
    main()
