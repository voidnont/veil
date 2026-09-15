import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class GeckoReleaseWorkflowTests(unittest.TestCase):
    def read(self, relative: str) -> str:
        return (ROOT / relative).read_text(encoding="utf-8")

    def test_release_workflow_uses_only_validated_new_engine_artifacts(self):
        workflow = self.read(".github/workflows/release.yml")

        self.assertIn("Veil 0.9.0 Release", workflow)
        self.assertIn("[release-0.9.0]", workflow)
        self.assertIn("contents: write", workflow)
        self.assertIn("actions: read", workflow)

        for artifact in (
            "veil-gecko-windows-package",
            "veil-gecko-linux-package",
            "veil-gecko-macos-package",
            "veil-android-debug-apk",
            "veil-ios-simulator-app",
        ):
            with self.subTest(artifact=artifact):
                self.assertIn(artifact, workflow)

        self.assertIn('gh release create "v${VERSION}"', workflow)
        self.assertIn("SHA256SUMS.txt", workflow)
        self.assertIn("physical-device/App Store IPA", workflow)
        self.assertNotIn("0.8.9", workflow)
        self.assertNotIn("veil-windows-installer", workflow)
        self.assertNotIn("target/release/veil-browser", workflow)

    def test_release_requires_successful_artifact_runs_before_publish(self):
        workflow = self.read(".github/workflows/release.yml")
        self.assertIn("status=success", workflow)
        self.assertIn("required_gecko_artifacts", workflow)
        self.assertIn("required_app_artifacts", workflow)
        self.assertIn("No successful full Gecko run with all required release artifacts was found", workflow)
        self.assertIn("No successful Veil CI run with the iOS release artifact was found", workflow)

    def test_desktop_release_assets_get_platform_specific_names(self):
        workflow = self.read(".github/workflows/release.yml")
        self.assertIn("copy_platform_artifacts()", workflow)
        for platform in ("Windows", "Linux", "macOS"):
            with self.subTest(platform=platform):
                self.assertIn(f'Veil-${{VERSION}}-{platform}-', workflow)
        self.assertIn("No ${platform} package files were present in the validated artifact", workflow)
        self.assertNotIn("-exec cp -f {} dist/", workflow)


if __name__ == "__main__":
    unittest.main()
