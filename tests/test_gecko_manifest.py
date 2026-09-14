import subprocess
import tempfile
import unittest
from pathlib import Path

from tools.gecko_manifest import (
    ManifestError,
    read_patch_series,
    read_revision,
    verify_checkout,
    verify_manifest,
)


class GeckoManifestTests(unittest.TestCase):
    def make_repo(self) -> Path:
        temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(temp_dir.cleanup)
        root = Path(temp_dir.name)
        (root / "gecko" / "patches").mkdir(parents=True)
        return root

    def test_revision_must_be_one_lowercase_full_sha(self):
        root = self.make_repo()
        (root / "gecko" / "REVISION").write_text("ABC123\n", encoding="utf-8")
        with self.assertRaisesRegex(ManifestError, "40-character lowercase hexadecimal"):
            read_revision(root)

    def test_revision_rejects_extra_nonempty_lines(self):
        root = self.make_repo()
        (root / "gecko" / "REVISION").write_text(
            "9e4ba5f8a056a91000b369dd508c1e438e3a2192\nextra\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(ManifestError, "exactly one non-empty line"):
            read_revision(root)

    def test_patch_series_ignores_blank_lines_and_comments(self):
        root = self.make_repo()
        (root / "gecko" / "patches" / "series").write_text(
            "# Veil patch order\n\nbranding.patch\n  # note\nprivacy.patch\n",
            encoding="utf-8",
        )
        (root / "gecko" / "patches" / "branding.patch").write_text("x", encoding="utf-8")
        (root / "gecko" / "patches" / "privacy.patch").write_text("x", encoding="utf-8")
        self.assertEqual(read_patch_series(root), ["branding.patch", "privacy.patch"])

    def test_manifest_rejects_duplicate_patch_entries(self):
        root = self.make_repo()
        (root / "gecko" / "REVISION").write_text(
            "9e4ba5f8a056a91000b369dd508c1e438e3a2192\n",
            encoding="utf-8",
        )
        (root / "gecko" / "patches" / "series").write_text(
            "branding.patch\nbranding.patch\n", encoding="utf-8"
        )
        (root / "gecko" / "patches" / "branding.patch").write_text("x", encoding="utf-8")
        with self.assertRaisesRegex(ManifestError, "duplicate patch"):
            verify_manifest(root)

    def test_manifest_rejects_missing_listed_patch(self):
        root = self.make_repo()
        (root / "gecko" / "REVISION").write_text(
            "9e4ba5f8a056a91000b369dd508c1e438e3a2192\n",
            encoding="utf-8",
        )
        (root / "gecko" / "patches" / "series").write_text("missing.patch\n", encoding="utf-8")
        with self.assertRaisesRegex(ManifestError, "listed patch does not exist"):
            verify_manifest(root)

    def test_manifest_rejects_unlisted_patch_file(self):
        root = self.make_repo()
        (root / "gecko" / "REVISION").write_text(
            "9e4ba5f8a056a91000b369dd508c1e438e3a2192\n",
            encoding="utf-8",
        )
        (root / "gecko" / "patches" / "series").write_text("", encoding="utf-8")
        (root / "gecko" / "patches" / "surprise.patch").write_text("x", encoding="utf-8")
        with self.assertRaisesRegex(ManifestError, "unlisted patch"):
            verify_manifest(root)

    def test_checkout_head_must_match_revision(self):
        root = self.make_repo()
        expected = "9e4ba5f8a056a91000b369dd508c1e438e3a2192"
        (root / "gecko" / "REVISION").write_text(expected + "\n", encoding="utf-8")
        (root / "gecko" / "patches" / "series").write_text("", encoding="utf-8")

        checkout = root / "checkout"
        checkout.mkdir()
        subprocess.run(["git", "init"], cwd=checkout, check=True, capture_output=True)
        subprocess.run(["git", "config", "user.name", "Veil Test"], cwd=checkout, check=True)
        subprocess.run(["git", "config", "user.email", "veil-test@example.invalid"], cwd=checkout, check=True)
        (checkout / "file.txt").write_text("test", encoding="utf-8")
        subprocess.run(["git", "add", "file.txt"], cwd=checkout, check=True)
        subprocess.run(["git", "commit", "-m", "test"], cwd=checkout, check=True, capture_output=True)

        with self.assertRaisesRegex(ManifestError, "checkout HEAD"):
            verify_checkout(root, checkout, require_clean=False)


if __name__ == "__main__":
    unittest.main()
