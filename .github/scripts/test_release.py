"""Regression checks for release gating and downloadable archives."""

import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from package_release import package_release
from release_check import check_release, main


class ReleaseCheckTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.manifest = self.root / "Cargo.toml"
        self.manifest.write_text('[package]\nversion = "0.1.0"\n')
        env = patch.dict(os.environ, {"GITHUB_REPOSITORY": "owner/repo"})
        env.start()
        self.addCleanup(env.stop)
        self.api = patch("release_check.subprocess.run")
        self.mock_run = self.api.start()
        self.addCleanup(self.api.stop)
        self.mock_run.return_value.stdout = "[[]]"

    def check(self, tag):
        return check_release(tag, self.manifest)

    def test_matching_tags_enable_release(self):
        for tag in ("0.1.0", "v0.1.0"):
            with self.subTest(tag=tag):
                self.assertEqual(self.check(tag)[:2], ("0.1.0", True))
        self.mock_run.assert_called_with(
            ["gh", "api", "--paginate", "--slurp", "repos/owner/repo/releases?per_page=100"],
            check=True, capture_output=True, text=True,
        )

    def test_mismatched_tags_skip_without_api_request(self):
        for tag in ("v0.2.0", "v0.1.0-rc.1", "release-0.1.0", "vv0.1.0", "v0.1.0\n"):
            with self.subTest(tag=tag):
                self.assertFalse(self.check(tag)[1])
        self.mock_run.assert_not_called()

    def test_existing_versions_skip_including_later_pages_and_drafts(self):
        for tag in ("0.1.0", "v0.1.0"):
            for existing in ("0.1.0", "v0.1.0"):
                for draft in (False, True):
                    with self.subTest(tag=tag, existing=existing, draft=draft):
                        self.mock_run.return_value.stdout = json.dumps([
                            [{"tag_name": "v0.0.1"}],
                            [{"tag_name": existing, "draft": draft}],
                        ])
                        self.assertFalse(self.check(tag)[1])

    def test_unrelated_releases_do_not_block(self):
        self.mock_run.return_value.stdout = '[[{"tag_name": "v0.0.1"}]]'
        self.assertTrue(self.check("v0.1.0")[1])

    def test_api_failure_is_not_permission_to_release(self):
        self.mock_run.side_effect = subprocess.CalledProcessError(1, "gh api")
        with self.assertRaises(subprocess.CalledProcessError):
            self.check("v0.1.0")

    def test_malformed_response_fails_closed(self):
        for response in ("not JSON", '{"message": "API error"}', '[[{}]]'):
            with self.subTest(response=response):
                self.mock_run.return_value.stdout = response
                with self.assertRaises((ValueError, TypeError, KeyError)):
                    self.check("v0.1.0")

    def test_main_outputs_and_prerelease_classification(self):
        output = self.root / "output"
        summary = self.root / "summary"
        for version, eligible, prerelease in (
            ("0.1.0", True, False),
            ("0.1.0-rc.1+build.2", True, True),
            ("0.1.0+build-with-hyphen", True, False),
            ("0.1.0", False, False),
        ):
            with self.subTest(version=version, eligible=eligible):
                output.write_text("")
                with patch.dict(os.environ, {
                    "GITHUB_REF_NAME": f"v{version}",
                    "GITHUB_OUTPUT": str(output),
                    "GITHUB_STEP_SUMMARY": str(summary),
                }), patch("release_check.check_release", return_value=(version, eligible, "reason")):
                    main()
                self.assertEqual(output.read_text(), (
                    f"version={version}\nshould_release={str(eligible).lower()}\n"
                    f"prerelease={str(prerelease).lower()}\n"
                ))
        self.assertEqual(summary.read_text(), "reason\n" * 4)


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in ("README.md", "LICENSE"):
            (self.root / name).write_text(f"test {name}")

    def test_all_platform_archives_include_binary_and_docs(self):
        for platform in ("linux", "macos", "windows"):
            for arch in ("amd64", "arm64"):
                with self.subTest(platform=platform, arch=arch):
                    target = f"test-{platform}-{arch}"
                    name = "affine-local-mcp.exe" if platform == "windows" else "affine-local-mcp"
                    binary = self.root / "target" / target / "release" / name
                    binary.parent.mkdir(parents=True)
                    binary.write_bytes(b"test executable")
                    binary.chmod(0o755)
                    archive = package_release(target, platform, arch, "0.1.0", self.root)
                    prefix = f"affine-local-mcp-0.1.0-{platform}-{arch}"
                    if platform == "windows":
                        self.assertEqual(archive.name, f"{prefix}.zip")
                        with zipfile.ZipFile(archive) as contents:
                            self.assertEqual(set(contents.namelist()), {name, "README.md", "LICENSE"})
                            self.assertEqual(contents.read(name), binary.read_bytes())
                            self.assertEqual(contents.read("LICENSE"), (self.root / "LICENSE").read_bytes())
                    else:
                        self.assertEqual(archive.name, f"{prefix}.tar.gz")
                        with tarfile.open(archive) as contents:
                            self.assertEqual(set(contents.getnames()), {name, "README.md", "LICENSE"})
                            executable = contents.extractfile(name)
                            license_file = contents.extractfile("LICENSE")
                            assert executable is not None and license_file is not None
                            with executable, license_file:
                                self.assertEqual(executable.read(), binary.read_bytes())
                                self.assertEqual(license_file.read(), (self.root / "LICENSE").read_bytes())
                            self.assertEqual(contents.getmember(name).mode & 0o111, 0o111)
        self.assertEqual(len(list((self.root / "dist").iterdir())), 6)

    def test_missing_binary_fails_without_creating_archive(self):
        with self.assertRaises(FileNotFoundError):
            package_release("missing-target", "linux", "amd64", "0.1.0", self.root)
        self.assertFalse((self.root / "dist").exists())


if __name__ == "__main__":
    unittest.main()
