"""Verify native macOS wheel tags with universal2 Python interpreters."""

from pathlib import Path
import runpy
import unittest
from unittest.mock import patch

from setuptools.command.bdist_wheel import bdist_wheel

ROOT = Path(__file__).resolve().parents[2]


class WheelTagTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Load the real build commands without executing setuptools' CLI.
        with patch("setuptools.setup"):
            cls.build = runpy.run_path(str(ROOT / "setup.py"))

    def tag(self, platform, machine):
        distribution = self.build["BinaryDistribution"]()
        wheel = self.build["BinaryWheel"](distribution)
        with patch.object(bdist_wheel, "get_tag", return_value=("cp313", "cp313", platform)), patch("platform.machine", return_value=machine):
            return wheel.get_tag()

    def test_universal2_python_produces_distinct_single_architecture_wheels(self):
        tags = [self.tag("macosx_11_0_universal2", arch) for arch in ("arm64", "x86_64")]
        self.assertEqual(tags, [
            ("py3", "none", "macosx_11_0_arm64"),
            ("py3", "none", "macosx_11_0_x86_64"),
        ])
        self.assertEqual(len(set(tags)), 2)

    def test_other_platform_tags_are_preserved(self):
        for platform in ("linux_aarch64", "win_arm64", "macosx_12_0_x86_64"):
            with self.subTest(platform=platform):
                self.assertEqual(self.tag(platform, "x86_64"), ("py3", "none", platform))

    def test_universal2_with_unknown_native_architecture_fails(self):
        with self.assertRaisesRegex(ValueError, "macOS architecture"):
            self.tag("macosx_11_0_universal2", "unknown")


if __name__ == "__main__":
    unittest.main()
