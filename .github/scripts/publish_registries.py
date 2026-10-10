"""Publish immutable release artifacts and verify their registry checksums."""

import argparse
import base64
from email.parser import BytesParser
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import time
import tomllib
import zipfile
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen

from package_registries import ROOT, metadata

NPM_REGISTRY = "https://registry.npmjs.org"
PYPI_UPLOAD = "https://upload.pypi.org/legacy/"
SECRETS = {
    "CARGO_REGISTRY_TOKEN": "CARGO_REGISTRY_TOKEN",
    "NODE_AUTH_TOKEN": "NPM_TOKEN",
    "TWINE_PASSWORD": "PYPI_API_TOKEN",
}


def require_secrets():
    missing = [secret for env, secret in SECRETS.items() if not os.environ.get(env, "").strip()]
    if missing:
        raise RuntimeError("Missing GitHub Actions secrets: " + ", ".join(missing))
    print("All three publishing secrets are configured (values not displayed).")


def digest(path, algorithm="sha256"):
    with path.open("rb") as source:
        return hashlib.file_digest(source, algorithm).digest()


def fetch_json(url):
    request = Request(url, headers={
        "Accept": "application/json",
        "User-Agent": "affine-desktop-mcp-release (https://github.com/sauravniraula/affine-desktop-mcp)",
    })
    try:
        with urlopen(request, timeout=30) as response:
            return json.load(response)
    except HTTPError as error:
        error.close()
        if error.code == 404:
            return None
        raise  # Auth failures, rate limits and outages are never "not published".


def wait_for_verification(check, label, attempts=20):
    for attempt in range(attempts):
        if check():
            print(f"Verified registry checksums: {label}")
            return
        if attempt + 1 < attempts:
            time.sleep(3)
    raise RuntimeError(f"Registry verification timed out: {label}; rerun the failed publish job")


def publish_one(check, command, label, dry_run=False):
    if dry_run:
        subprocess.run(command, cwd=ROOT, check=True)
        return
    if check():
        print(f"Already published with matching checksum: {label}")
        return
    subprocess.run(command, cwd=ROOT, check=True)
    wait_for_verification(check, label)


def publish_npm(dist, package, dry_run=False):
    version = package["version"]
    # The launcher must not become available before all its native dependencies.
    names = [*sorted(package["optionalDependencies"]), package["name"]]
    artifacts = []
    for name in names:
        archive = dist / f"{name}-{version}.tgz"
        with tarfile.open(archive) as source:
            member = source.extractfile("package/package.json")
            if member is None:
                raise ValueError(f"Missing package.json: {archive}")
            with member:
                packed = json.load(member)
        if packed["name"] != name or packed["version"] != version:
            raise ValueError(f"Wrong npm name/version in {archive}")
        integrity = "sha512-" + base64.b64encode(digest(archive, "sha512")).decode()
        artifacts.append((name, archive, integrity))
    npm = shutil.which("npm")
    if npm is None:
        raise RuntimeError("npm is required")
    tag = "next" if "-" in version.split("+", 1)[0] else "latest"
    for name, archive, integrity in artifacts:
        def check(name=name, integrity=integrity):
            remote = fetch_json(f"{NPM_REGISTRY}/{quote(name, safe='')}/{quote(version, safe='')}")
            if remote is None:
                return False
            if remote["name"] != name or remote["version"] != version or remote["dist"]["integrity"] != integrity:
                raise ValueError(f"Conflicting npm release: {name}@{version}")
            return True

        command = [npm, "publish", str(archive), "--access", "public", "--tag", tag,
                   "--registry", NPM_REGISTRY, "--ignore-scripts"]
        if dry_run:
            command.append("--dry-run")
        publish_one(check, command, f"{name}@{version}", dry_run)


def publish_pypi(dist, version, dry_run=False):
    from packaging.version import Version

    version = str(Version(version))  # Cargo SemVer -> Python's normalized spelling.
    files = [*sorted(dist.glob("*.whl")), *sorted(dist.glob("affine_desktop_mcp-*.tar.gz"))]
    if len(files) != 7 or len(list(dist.glob("*.whl"))) != 6:
        raise ValueError("PyPI publication requires six wheels and one source distribution")
    for path in files:
        if path.suffix == ".whl":
            with zipfile.ZipFile(path) as archive:
                candidates = [name for name in archive.namelist() if name.endswith(".dist-info/METADATA")]
                if len(candidates) != 1:
                    raise ValueError(f"Expected one wheel metadata file: {path.name}")
                content = archive.read(candidates[0])
        else:
            with tarfile.open(path) as archive:
                candidates = [member for member in archive.getmembers() if member.name.endswith("/PKG-INFO") and member.name.count("/") == 1]
                if len(candidates) != 1:
                    raise ValueError(f"Expected one source metadata file: {path.name}")
                member = archive.extractfile(candidates[0])
                if member is None:
                    raise ValueError(f"Missing source metadata: {path.name}")
                with member:
                    content = member.read()
        info = BytesParser().parsebytes(content)
        if info["Name"] != "affine-desktop-mcp" or Version(info["Version"]) != Version(version):
            raise ValueError(f"Wrong Python name/version in {path.name}")
    expected = {path.name: digest(path).hex() for path in files}
    url = f"https://pypi.org/pypi/affine-desktop-mcp/{quote(version, safe='')}/json"

    def pending():
        remote = fetch_json(url)
        if remote is None:
            return files
        if Version(remote["info"]["version"]) != Version(version):
            raise ValueError("Wrong PyPI release version")
        published = {item["filename"]: item for item in remote["urls"]}
        missing = []
        for path in files:
            item = published.get(path.name)
            if item is None:
                missing.append(path)
            elif item["digests"]["sha256"] != expected[path.name] or item["yanked"]:
                raise ValueError(f"Conflicting or yanked PyPI file: {path.name}")
        return missing

    if dry_run:
        subprocess.run([sys.executable, "-m", "twine", "check", *map(str, files)], cwd=ROOT, check=True)
        return
    missing = pending()
    if missing:
        subprocess.run([sys.executable, "-m", "twine", "upload", "--non-interactive",
                        "--repository-url", PYPI_UPLOAD, *map(str, missing)], cwd=ROOT, check=True)
    wait_for_verification(lambda: not pending(), f"affine-desktop-mcp=={version}")


def publish_cargo(dist, version, dry_run=False):
    with (ROOT / "Cargo.toml").open("rb") as source:
        name = tomllib.load(source)["package"]["name"]
    archive = dist / f"{name}-{version}.crate"
    checksum = digest(archive).hex()

    def check():
        remote = fetch_json(f"https://crates.io/api/v1/crates/{quote(name, safe='')}/{quote(version, safe='')}")
        if remote is None:
            return False
        published = remote["version"]
        if published["crate"] != name or published["num"] != version or published["checksum"] != checksum or published["yanked"]:
            raise ValueError(f"Conflicting or yanked Cargo release: {name}@{version}")
        return True

    if not dry_run and check():
        print(f"Already published with matching checksum: {name}@{version}")
        return
    # Cargo publishes from source. Recreate the archive from this exact tag and
    # ensure it is byte-identical to the dry-run-verified GitHub release asset.
    subprocess.run(["cargo", "package", "--locked", "--no-verify", "--registry", "crates-io"], cwd=ROOT, check=True)
    cargo = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT, text=True))
    built = Path(cargo["target_directory"]) / "package" / archive.name
    if digest(built).hex() != checksum:
        raise ValueError("Cargo source archive differs from the verified release asset")
    command = ["cargo", "publish", "--locked", "--registry", "crates-io"]
    if dry_run:
        command.append("--dry-run")
    publish_one(check, command, f"{name}@{version}", dry_run)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-secrets", action="store_true")
    parser.add_argument("--registry", choices=("cargo", "npm", "pypi"))
    parser.add_argument("--dry-run", action="store_true", help="Validate packages without uploading")
    parser.add_argument("--tag", help="Require a v-prefixed tag matching the package version")
    args = parser.parse_args()
    if args.tag and args.tag != f"v{metadata()['version']}":
        raise ValueError("Release tag does not match the package manifests")
    if args.check_secrets:
        require_secrets()
        return
    if not args.registry:
        parser.error("--registry is required unless --check-secrets is used")
    package = metadata()
    dist = ROOT / "dist"
    if args.registry == "npm":
        publish_npm(dist, package, args.dry_run)
    elif args.registry == "pypi":
        publish_pypi(dist, package["version"], args.dry_run)
    else:
        publish_cargo(dist, package["version"], args.dry_run)


if __name__ == "__main__":
    main()
