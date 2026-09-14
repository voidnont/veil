import tempfile
import unittest
from pathlib import Path

from tools.gecko_branding import BrandingError, apply_branding, validate_overlay


ROOT = Path(__file__).resolve().parents[1]


class GeckoStageBIdentityTests(unittest.TestCase):
    def test_checked_in_branding_uses_only_veil_identity(self):
        validate_overlay(ROOT)

        configure = (ROOT / "gecko" / "branding" / "configure.sh").read_text(encoding="utf-8")
        self.assertIn("MOZ_APP_DISPLAYNAME=Veil", configure)
        self.assertIn("MOZ_APP_BASENAME=Veil", configure)
        self.assertIn("MOZ_APP_VENDOR=Veil", configure)
        self.assertIn("MOZ_MACBUNDLE_ID=app.veil.browser", configure)

        fluent = (ROOT / "gecko" / "branding" / "locales" / "en-US" / "brand.ftl").read_text(encoding="utf-8")
        self.assertIn("-brand-short-name = Veil", fluent)
        self.assertIn("-brand-full-name = Veil Browser", fluent)
        self.assertIn("-brand-product-name = Veil", fluent)
        self.assertIn("-vendor-short-name = Veil", fluent)

        combined = configure + fluent + (
            ROOT / "gecko" / "branding" / "locales" / "en-US" / "brand.properties"
        ).read_text(encoding="utf-8")
        for forbidden in ("Firefox", "Nightly", "Mozilla"):
            self.assertNotIn(forbidden, combined)

    def test_overlay_is_created_from_upstream_and_replaces_product_pngs(self):
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp) / "gecko"
            unofficial = source / "browser" / "branding" / "unofficial"
            (unofficial / "locales" / "en-US").mkdir(parents=True)
            (unofficial / "configure.sh").write_text("MOZ_APP_DISPLAYNAME=Nightly\n", encoding="utf-8")
            (unofficial / "locales" / "en-US" / "brand.ftl").write_text(
                "-brand-short-name = Nightly\n", encoding="utf-8"
            )
            (unofficial / "locales" / "en-US" / "brand.properties").write_text(
                "brandShortName=Nightly\n", encoding="utf-8"
            )
            (unofficial / "default16.png").write_bytes(b"upstream-icon")
            (unofficial / "VisualElements_70.png").write_bytes(b"upstream-tile")
            (unofficial / "keep.txt").write_text("upstream", encoding="utf-8")

            destination = apply_branding(ROOT, source)

            self.assertEqual((destination / "keep.txt").read_text(encoding="utf-8"), "upstream")
            self.assertEqual(
                (destination / "configure.sh").read_text(encoding="utf-8"),
                (ROOT / "gecko" / "branding" / "configure.sh").read_text(encoding="utf-8"),
            )
            veil_icon = (ROOT / "assets" / "veil-glass-icon.png").read_bytes()
            self.assertEqual((destination / "default16.png").read_bytes(), veil_icon)
            self.assertEqual((destination / "VisualElements_70.png").read_bytes(), veil_icon)

    def test_missing_upstream_branding_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            with self.assertRaisesRegex(BrandingError, "upstream unofficial branding"):
                apply_branding(ROOT, Path(temp))


if __name__ == "__main__":
    unittest.main()
