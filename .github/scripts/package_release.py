"""Package a built executable with its documentation and license."""

import argparse
from pathlib import Path
import tarfile
import zipfile


def package_release(target, platform, arch, version, root=Path(".")):
    binary = "affine-local-mcp.exe" if platform == "windows" else "affine-local-mcp"
    files = [
        root / "target" / target / "release" / binary,
        root / "README.md",
        root / "LICENSE",
    ]
    for source in files:
        if not source.is_file():
            raise FileNotFoundError(source)
    dist = root / "dist"
    dist.mkdir(exist_ok=True)
    name = f"affine-local-mcp-{version}-{platform}-{arch}"
    if platform == "windows":
        archive = dist / f"{name}.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
            for source in files:
                output.write(source, arcname=source.name)
    else:
        archive = dist / f"{name}.tar.gz"
        with tarfile.open(archive, "w:gz") as output:
            for source in files:
                output.add(source, arcname=source.name)
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--platform", required=True, choices=("linux", "macos", "windows"))
    parser.add_argument("--arch", required=True, choices=("amd64", "arm64"))
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    print(package_release(args.target, args.platform, args.arch, args.version))


if __name__ == "__main__":
    main()
