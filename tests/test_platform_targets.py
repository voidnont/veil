import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class PlatformTargetTests(unittest.TestCase):
    def read(self, relative: str) -> str:
        return (ROOT / relative).read_text(encoding="utf-8")

    def test_macos_is_a_supported_desktop_target(self):
        mozconfig = self.read("gecko/mozconfig.macos")
        self.assertIn("ac_add_options --enable-application=browser", mozconfig)
        self.assertIn("ac_add_options --with-branding=browser/branding/veil", mozconfig)
        self.assertIn("ac_add_options --with-distribution-id=app.veil", mozconfig)

        build_script = self.read("scripts/gecko-build.sh")
        self.assertIn("Darwin", build_script)
        self.assertIn("mozconfig.macos", build_script)

        gecko_ci = self.read(".github/workflows/gecko-stage-a.yml")
        self.assertIn("macos-full-build:", gecko_ci)
        self.assertIn("Full Gecko build - macOS", gecko_ci)

        app_ci = self.read(".github/workflows/veil.yml")
        self.assertIn("macos-latest", app_ci)

    def test_android_uses_pinned_geckoview_and_builds_an_apk(self):
        mozconfig = self.read("gecko/mozconfig.android")
        self.assertIn("ac_add_options --enable-application=mobile/android", mozconfig)
        self.assertIn("ac_add_options --target=aarch64-linux-android", mozconfig)

        build_script = self.read("scripts/gecko-build-android.sh")
        self.assertIn("GeckoView/Firefox for Android", build_script)
        self.assertIn("mach build", build_script)
        self.assertIn("publishWithGeckoBinariesDebugPublicationToMavenRepository", build_script)
        self.assertIn("archive-geckoview", build_script)
        self.assertIn('rootProject.name = "veil-gecko-android"', build_script)

        gradle = self.read("mobile/android/app/build.gradle")
        self.assertIn("substitute-local-geckoview.gradle", gradle)
        self.assertIn("org.mozilla.geckoview:geckoview-nightly", gradle)

        activity = self.read("mobile/android/app/src/main/java/app/veil/browser/MainActivity.java")
        self.assertIn("GeckoRuntime", activity)
        self.assertIn("GeckoSession", activity)
        self.assertIn("GeckoView", activity)

        manifest = self.read("mobile/android/app/src/main/AndroidManifest.xml")
        self.assertIn("android.permission.INTERNET", manifest)

        gecko_ci = self.read(".github/workflows/gecko-stage-a.yml")
        self.assertIn("android-geckoview-build:", gecko_ci)
        self.assertIn("veil-android-debug-apk", gecko_ci)

    def test_ios_has_a_native_webkit_browser_target(self):
        project = self.read("mobile/ios/project.yml")
        self.assertIn("platform: iOS", project)
        self.assertIn("PRODUCT_BUNDLE_IDENTIFIER: app.veil.browser.ios", project)

        app = self.read("mobile/ios/Sources/VeilApp.swift")
        self.assertIn("@main", app)
        self.assertIn("BrowserView", app)

        browser = self.read("mobile/ios/Sources/BrowserView.swift")
        self.assertIn("WKWebView", browser)
        self.assertIn("WKNavigationDelegate", browser)

        ci = self.read(".github/workflows/veil.yml")
        self.assertIn("ios-build:", ci)
        self.assertIn("xcodegen", ci)
        self.assertIn("xcodebuild", ci)


if __name__ == "__main__":
    unittest.main()
