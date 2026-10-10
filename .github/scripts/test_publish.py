"""Publication regression tests; all registry writes are mocked."""

import base64
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError, URLError
from email.message import Message
import zipfile

from package_registries import metadata
from publish_registries import (
    digest, fetch_json, publish_cargo, publish_npm, publish_pypi,
    require_secrets, wait_for_verification, main,
)


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.dist = Path(self.temp.name)
        self.package = metadata()
        self.version = "0.1.0"
        self.package["version"] = self.version

    def tar(self, path, name, content):
        with tarfile.open(path, "w:gz") as archive:
            item = tarfile.TarInfo(name)
            item.size = len(content)
            archive.addfile(item, io.BytesIO(content))

    def npm_files(self, version=None):
        version = version or self.version
        names = [*sorted(self.package["optionalDependencies"]), self.package["name"]]
        for name in names:
            self.tar(self.dist / f"{name}-{version}.tgz", "package/package.json",
                     json.dumps({"name": name, "version": version}).encode())
        return names

    def python_files(self, version=None):
        version = version or self.version
        content = f"Metadata-Version: 2.4\nName: affine-desktop-mcp\nVersion: {version}\n".encode()
        files = []
        for index in range(6):
            path = self.dist / f"affine_desktop_mcp-{version}-py3-none-test_{index}.whl"
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr(f"affine_desktop_mcp-{version}.dist-info/METADATA", content)
            files.append(path)
        source = self.dist / f"affine_desktop_mcp-{version}.tar.gz"
        self.tar(source, f"affine_desktop_mcp-{version}/PKG-INFO", content)
        return [*files, source]

    def python_remote(self, files):
        return {"info": {"version": self.version}, "urls": [
            {"filename": path.name, "digests": {"sha256": digest(path).hex()}, "yanked": False}
            for path in files
        ]}

    def test_secret_preflight_reports_names_not_values(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(RuntimeError, "CARGO_REGISTRY_TOKEN, NPM_TOKEN, PYPI_API_TOKEN"):
                require_secrets()
        with patch.dict(os.environ, {
            "CARGO_REGISTRY_TOKEN": "test-only", "NODE_AUTH_TOKEN": "test-only", "TWINE_PASSWORD": "test-only",
        }), patch("builtins.print") as output:
            require_secrets()
            self.assertNotIn("test-only", str(output.call_args))

    def test_wrong_recovery_tag_fails_before_secret_check_or_publish(self):
        with patch("sys.argv", ["publish_registries.py", "--check-secrets", "--tag", "v99.0.0"]), patch("publish_registries.require_secrets") as secrets:
            with self.assertRaisesRegex(ValueError, "tag does not match"):
                main()
            secrets.assert_not_called()

    def test_only_http_404_means_absent(self):
        for status in (401, 403, 429, 500):
            with self.subTest(status=status), patch("publish_registries.urlopen", side_effect=HTTPError("url", status, "failed", Message(), io.BytesIO())):
                with self.assertRaises(HTTPError):
                    fetch_json("https://example.test")
        with patch("publish_registries.urlopen", side_effect=HTTPError("url", 404, "absent", Message(), io.BytesIO())):
            self.assertIsNone(fetch_json("https://example.test"))
        with patch("publish_registries.urlopen", side_effect=URLError("offline")):
            with self.assertRaises(URLError):
                fetch_json("https://example.test")
        with patch("publish_registries.urlopen") as open_url:
            open_url.return_value.__enter__.return_value = io.BytesIO(b"not json")
            with self.assertRaises(ValueError):
                fetch_json("https://example.test")

    def test_npm_publishes_all_natives_before_launcher_and_verifies_each(self):
        names = self.npm_files()
        published = {}

        def upload(command, **kwargs):
            path = Path(command[2])
            with tarfile.open(path) as archive:
                member = archive.extractfile("package/package.json")
                assert member is not None
                with member:
                    packed = json.load(member)
            name = packed["name"]
            published[name] = {**packed, "dist": {"integrity": "sha512-" + base64.b64encode(digest(path, "sha512")).decode()}}

        with patch("publish_registries.fetch_json", side_effect=lambda url: published.get(url.split("/")[-2])), patch("publish_registries.subprocess.run", side_effect=upload) as run:
            publish_npm(self.dist, self.package)
            commands = [call.args[0] for call in run.call_args_list]
            self.assertEqual([Path(command[2]).name for command in commands], [f"{name}-{self.version}.tgz" for name in names])
            self.assertTrue(all(command[command.index("--tag") + 1] == "latest" for command in commands))
            self.assertTrue(all("--ignore-scripts" in command for command in commands))
            run.reset_mock()
            publish_npm(self.dist, self.package)
            run.assert_not_called()

    def test_npm_conflict_and_upload_failure_do_not_publish_launcher(self):
        self.npm_files()
        with patch("publish_registries.fetch_json", return_value={"name": "wrong", "version": self.version}), patch("publish_registries.subprocess.run") as run:
            with self.assertRaisesRegex(ValueError, "Conflicting npm"):
                publish_npm(self.dist, self.package)
            run.assert_not_called()
        with patch("publish_registries.fetch_json", return_value=None), patch("publish_registries.subprocess.run", side_effect=subprocess.CalledProcessError(1, "npm")) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                publish_npm(self.dist, self.package)
            self.assertEqual(run.call_count, 1)

    def test_npm_prerelease_dry_run_uses_next_without_registry_writes(self):
        package = {**self.package, "version": "0.2.0-rc.1"}
        self.npm_files(package["version"])
        with patch("publish_registries.fetch_json") as fetch, patch("publish_registries.subprocess.run") as run:
            publish_npm(self.dist, package, dry_run=True)
            fetch.assert_not_called()
            self.assertEqual(run.call_count, 7)
            for call in run.call_args_list:
                command = call.args[0]
                self.assertIn("--dry-run", command)
                self.assertEqual(command[command.index("--tag") + 1], "next")

    def test_pypi_partial_retry_uploads_only_missing_files_and_verifies(self):
        files = self.python_files()
        with patch("publish_registries.fetch_json", side_effect=[self.python_remote(files[:2]), self.python_remote(files)]), patch("publish_registries.subprocess.run") as run:
            publish_pypi(self.dist, self.version)
            command = run.call_args.args[0]
            self.assertIn("--non-interactive", command)
            self.assertEqual(set(command[command.index("--repository-url") + 2:]), set(map(str, files[2:])))
        with patch("publish_registries.fetch_json", return_value=self.python_remote(files)), patch("publish_registries.subprocess.run") as run:
            publish_pypi(self.dist, self.version)
            run.assert_not_called()

    def test_pypi_conflicts_yanks_and_wrong_local_versions_fail_before_upload(self):
        files = self.python_files()
        for field, value in (("digests", {"sha256": "wrong"}), ("yanked", True)):
            remote = self.python_remote(files)
            remote["urls"][0][field] = value
            with patch("publish_registries.fetch_json", return_value=remote), patch("publish_registries.subprocess.run") as run:
                with self.assertRaisesRegex(ValueError, "Conflicting or yanked"):
                    publish_pypi(self.dist, self.version)
                run.assert_not_called()
        with patch("publish_registries.subprocess.run") as run:
            with self.assertRaisesRegex(ValueError, "Wrong Python name/version"):
                publish_pypi(self.dist, "99.0.0")
            run.assert_not_called()

    def test_cargo_matching_existing_version_skips_but_conflict_fails(self):
        path = self.dist / f"affine-desktop-mcp-{self.version}.crate"
        path.write_bytes(b"crate checksum fixture")
        remote = {"version": {"crate": "affine-desktop-mcp", "num": self.version, "checksum": digest(path).hex(), "yanked": False}}
        with patch("publish_registries.fetch_json", return_value=remote) as fetch, patch("publish_registries.subprocess.run") as run:
            publish_cargo(self.dist, self.version)
            fetch.assert_called_once_with(f"https://crates.io/api/v1/crates/affine-desktop-mcp/{self.version}")
            run.assert_not_called()
            remote["version"]["checksum"] = "wrong"
            with self.assertRaisesRegex(ValueError, "Conflicting or yanked Cargo"):
                publish_cargo(self.dist, self.version)
            run.assert_not_called()

    def test_cargo_verifies_source_archive_before_upload_and_readback(self):
        path = self.dist / f"affine-desktop-mcp-{self.version}.crate"
        path.write_bytes(b"crate checksum fixture")
        target = self.dist / "target"
        built = target / "package" / path.name
        built.parent.mkdir(parents=True)
        built.write_bytes(path.read_bytes())
        remote = {"version": {"crate": "affine-desktop-mcp", "num": self.version, "checksum": digest(path).hex(), "yanked": False}}
        with patch("publish_registries.fetch_json", side_effect=[None, None, remote]), patch("publish_registries.subprocess.check_output", return_value=json.dumps({"target_directory": str(target)})), patch("publish_registries.subprocess.run") as run:
            publish_cargo(self.dist, self.version)
            self.assertEqual([call.args[0][1] for call in run.call_args_list], ["package", "publish"])
        built.write_bytes(b"wrong source archive")
        with patch("publish_registries.fetch_json", return_value=None), patch("publish_registries.subprocess.check_output", return_value=json.dumps({"target_directory": str(target)})), patch("publish_registries.subprocess.run") as run:
            with self.assertRaisesRegex(ValueError, "differs from"):
                publish_cargo(self.dist, self.version)
            self.assertEqual(run.call_count, 1)

    def test_verification_is_bounded_and_does_not_retry_conflicts(self):
        with patch("publish_registries.time.sleep") as sleep:
            with self.assertRaisesRegex(RuntimeError, "timed out"):
                wait_for_verification(lambda: False, "fixture", attempts=3)
            self.assertEqual(sleep.call_count, 2)
            sleep.reset_mock()
            def conflict():
                raise ValueError("conflict")
            with self.assertRaisesRegex(ValueError, "conflict"):
                wait_for_verification(conflict, "fixture")
            sleep.assert_not_called()


if __name__ == "__main__":
    unittest.main()
