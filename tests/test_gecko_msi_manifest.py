import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

from gecko_msi_manifest import build_manifest

NS = {"w": "http://wixtoolset.org/schemas/v4/wxs"}


class GeckoMsiManifestTests(unittest.TestCase):
    def test_manifest_packages_every_runtime_file_and_shortcuts_browser(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            runtime = Path(temp_dir) / "runtime"
            (runtime / "browser").mkdir(parents=True)
            (runtime / "firefox.exe").write_bytes(b"MZtest")
            (runtime / "xul.dll").write_bytes(b"dll")
            (runtime / "browser" / "omni.ja").write_bytes(b"jar")
            output = Path(temp_dir) / "veil.wxs"

            build_manifest(runtime, output, version="0.9.0")

            tree = ET.parse(output)
            package = tree.getroot().find("w:Package", NS)
            self.assertIsNotNone(package)
            self.assertEqual(package.attrib["Version"], "0.9.0")

            files = package.findall(".//w:File", NS)
            component_refs = package.findall(".//w:Feature/w:ComponentRef", NS)
            self.assertEqual(len(files), 3)
            self.assertEqual(len(component_refs), 3)

            shortcuts = [
                shortcut.attrib.get("Name")
                for shortcut in package.findall(".//w:Shortcut", NS)
            ]
            self.assertIn("Veil Browser", shortcuts)
            self.assertTrue(
                any(file.attrib["Source"].endswith("firefox.exe") for file in files)
            )

    def test_manifest_rejects_runtime_without_browser_executable(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            runtime = Path(temp_dir) / "runtime"
            runtime.mkdir()
            (runtime / "xul.dll").write_bytes(b"dll")

            with self.assertRaisesRegex(ValueError, "browser executable"):
                build_manifest(runtime, Path(temp_dir) / "veil.wxs")


if __name__ == "__main__":
    unittest.main()
