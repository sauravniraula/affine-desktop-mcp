"""Build Python wheels containing the Rust executable, not a Python extension."""

import os
import json
from pathlib import Path
import shutil
import subprocess
import tomllib

from setuptools import Distribution, setup
from setuptools.command.build_py import build_py
from setuptools.command.bdist_wheel import bdist_wheel

ROOT = Path(__file__).resolve().parent
with (ROOT / "Cargo.toml").open("rb") as source:
    VERSION = tomllib.load(source)["package"]["version"]


class BinaryDistribution(Distribution):
    def has_ext_modules(self):
        # Use platlib rather than purelib for bundled native executables. This
        # also allows auditwheel to inspect and repair the Linux wheel.
        return True


class BuildBinary(build_py):
    def run(self):
        super().run()
        executable = "affine-local-mcp.exe" if os.name == "nt" else "affine-local-mcp"
        prebuilt = os.environ.get("AFFINE_DESKTOP_MCP_BINARY")
        if prebuilt:
            binary = Path(prebuilt).resolve()
        else:
            output = subprocess.check_output(
                ["cargo", "build", "--release", "--locked", "--bin", "affine-local-mcp", "--message-format=json"],
                cwd=ROOT, text=True,
            )
            artifacts = [json.loads(line) for line in output.splitlines()]
            binaries = [
                artifact["executable"]
                for artifact in artifacts
                if artifact.get("reason") == "compiler-artifact"
                and artifact["target"]["name"] == "affine-local-mcp"
                and artifact.get("executable")
            ]
            if len(binaries) != 1:
                raise RuntimeError("Cargo did not report exactly one affine-local-mcp executable")
            # Cargo's own artifact path honors CARGO_TARGET_DIR, configured
            # targets, and user Cargo configuration; do not assume ./target.
            binary = Path(binaries[0])
        # Prevent accidentally packaging an old cached binary. Wheels are built
        # natively on each supported runner, so the binary must be executable here.
        actual = subprocess.check_output([str(binary), "--version"], text=True).strip()
        if actual != VERSION:
            raise ValueError(f"Binary version {actual!r} does not match Cargo version {VERSION!r}")
        destination = Path(self.build_lib) / "affine_desktop_mcp" / "bin" / executable
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(binary, destination)
        destination.chmod(0o755)


class BinaryWheel(bdist_wheel):
    def finalize_options(self):
        super().finalize_options()
        self.root_is_pure = False

    def get_tag(self):
        _, _, platform = super().get_tag()
        # The launcher works across Python versions and the Rust executable has
        # no CPython ABI dependency. Never label a native wheel as "any".
        return "py3", "none", platform


setup(
    version=VERSION,
    distclass=BinaryDistribution,
    cmdclass={"build_py": BuildBinary, "bdist_wheel": BinaryWheel},
)
