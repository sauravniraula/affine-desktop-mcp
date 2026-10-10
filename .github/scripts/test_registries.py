"""Regression checks for package metadata and all six native npm packages."""

import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

from package_registries import ROOT, PLATFORMS, ARCHITECTURES, metadata, package_npm_binary


class RegistryPackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in ("Cargo.toml", "package.json", "pyproject.toml", "README.md", "LICENSE"):
            shutil.copy2(ROOT / name, self.root / name)

    def test_metadata_is_synchronized(self):
        self.assertEqual(metadata(self.root)["name"], "affine-desktop-mcp")

    def test_cargo_name_drift_is_rejected(self):
        manifest = self.root / "Cargo.toml"
        manifest.write_text(manifest.read_text().replace('name = "affine-desktop-mcp"', 'name = "wrong-name"', 1))
        with self.assertRaisesRegex(ValueError, "Cargo package name"):
            metadata(self.root)

    def test_version_drift_is_rejected(self):
        for field in ("version", "optionalDependencies"):
            with self.subTest(field=field):
                package = metadata()
                if field == "version":
                    package[field] = "99.0.0"
                else:
                    package[field]["affine-desktop-mcp-linux-x64"] = "99.0.0"
                (self.root / "package.json").write_text(json.dumps(package))
                with self.assertRaises(ValueError):
                    metadata(self.root)

    def test_all_native_packages_have_platform_constraints_and_binary(self):
        for platform, node_platform in PLATFORMS.items():
            for arch, node_arch in ARCHITECTURES.items():
                with self.subTest(platform=platform, arch=arch):
                    name = "affine-local-mcp.exe" if platform == "windows" else "affine-local-mcp"
                    binary = self.root / name
                    binary.write_bytes(b"packaging fixture; not used as an executable")

                    def inspect(staging, dist):
                        package = json.loads((staging / "package.json").read_text())
                        self.assertEqual(package["name"], f"affine-desktop-mcp-{node_platform}-{node_arch}")
                        self.assertEqual(package["os"], [node_platform])
                        self.assertEqual(package["cpu"], [node_arch])
                        self.assertEqual(package["version"], metadata()["version"])
                        self.assertEqual(package.get("libc"), ["glibc"] if platform == "linux" else None)
                        self.assertEqual((staging / "bin" / name).read_bytes(), binary.read_bytes())
                        if os.name != "nt":
                            self.assertEqual((staging / "bin" / name).stat().st_mode & 0o777, 0o755)
                        for document in ("README.md", "LICENSE"):
                            self.assertEqual((staging / document).read_bytes(), (self.root / document).read_bytes())
                        self.assertEqual(dist, self.root / "dist")

                    with patch("package_registries.pack_npm", side_effect=inspect) as pack:
                        package_npm_binary(binary, platform, arch, self.root)
                    pack.assert_called_once()

    def test_missing_binary_is_rejected(self):
        with self.assertRaises(ValueError):
            package_npm_binary(self.root / "affine-local-mcp", "linux", "amd64", self.root)


if __name__ == "__main__":
    unittest.main()
