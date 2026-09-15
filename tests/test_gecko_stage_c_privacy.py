import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

REQUIRED_PREFS = {
    'pref("datareporting.healthreport.uploadEnabled", false);',
    'pref("datareporting.policy.dataSubmissionEnabled", false);',
    'pref("toolkit.telemetry.enabled", false);',
    'pref("toolkit.telemetry.unified", false);',
    'pref("browser.newtabpage.activity-stream.telemetry", false);',
    'pref("browser.newtabpage.activity-stream.feeds.telemetry", false);',
    'pref("app.shield.optoutstudies.enabled", false);',
    'pref("browser.discovery.enabled", false);',
    'pref("privacy.globalprivacycontrol.enabled", true);',
    'pref("privacy.trackingprotection.enabled", true);',
    'pref("privacy.trackingprotection.pbmode.enabled", true);',
}

FORBIDDEN_PREF_NAMES = (
    "javascript.enabled",
    "media.peerconnection.enabled",
    "webgl.disabled",
    "dom.storage.enabled",
    "dom.serviceWorkers.enabled",
    "network.cookie.cookieBehavior",
    "privacy.resistFingerprinting",
)


class GeckoStageCPrivacyTests(unittest.TestCase):
    def read(self, relative: str) -> str:
        return (ROOT / relative).read_text(encoding="utf-8")

    def test_desktop_privacy_prefs_are_explicit_and_compatibility_safe(self):
        text = self.read("gecko/prefs/veil.js")
        for pref in REQUIRED_PREFS:
            self.assertIn(pref, text)
        for name in FORBIDDEN_PREF_NAMES:
            self.assertNotIn(name, text)

    def test_preference_overlay_preserves_upstream_branding_and_is_idempotent(self):
        from tools.gecko_prefs import apply_preferences

        with tempfile.TemporaryDirectory() as temp_dir:
            source = Path(temp_dir)
            destination = source / "browser/branding/veil/pref/firefox-branding.js"
            destination.parent.mkdir(parents=True)
            destination.write_text('pref("app.update.interval", 86400);\n', encoding="utf-8")

            apply_preferences(ROOT, source)
            first = destination.read_text(encoding="utf-8")
            apply_preferences(ROOT, source)
            second = destination.read_text(encoding="utf-8")

            self.assertIn('pref("app.update.interval", 86400);', first)
            self.assertIn("// Veil Stage C product privacy defaults", first)
            self.assertEqual(first, second)

    def test_bootstrap_applies_privacy_after_branding(self):
        shell = self.read("scripts/gecko-bootstrap.sh")
        powershell = self.read("scripts/gecko-bootstrap.ps1")
        self.assertIn("gecko_prefs.py", shell)
        self.assertIn("gecko_prefs.py", powershell)
        self.assertLess(shell.index("gecko_branding.py"), shell.index("gecko_prefs.py"))
        self.assertLess(powershell.index("gecko_branding.py"), powershell.index("gecko_prefs.py"))

    def test_android_runtime_privacy_defaults(self):
        text = self.read("mobile/android/app/src/main/java/app/veil/browser/MainActivity.java")
        for fragment in (
            "GeckoRuntimeSettings.Builder",
            ".globalPrivacyControlEnabled(true)",
            ".remoteDebuggingEnabled(false)",
            ".consoleOutput(false)",
            ".debugLogging(false)",
            "ContentBlocking.Settings.Builder",
            ".safeBrowsing(ContentBlocking.SafeBrowsing.DEFAULT)",
            ".enhancedTrackingProtectionLevel(ContentBlocking.EtpLevel.DEFAULT)",
        ):
            self.assertIn(fragment, text)

    def test_ios_webkit_privacy_defaults(self):
        helper = self.read("mobile/ios/Sources/PrivacyConfiguration.swift")
        browser = self.read("mobile/ios/Sources/BrowserView.swift")
        for fragment in (
            "configuration.websiteDataStore = .default()",
            "configuration.defaultWebpagePreferences.allowsContentJavaScript = true",
            "configuration.preferences.javaScriptCanOpenWindowsAutomatically = false",
            "webView.isInspectable = false",
        ):
            self.assertIn(fragment, helper)
        self.assertIn("PrivacyConfiguration.makeWebViewConfiguration()", browser)
        self.assertIn("PrivacyConfiguration.apply(to: webView)", browser)


if __name__ == "__main__":
    unittest.main()
