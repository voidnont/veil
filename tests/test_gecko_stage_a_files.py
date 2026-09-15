import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class GeckoStageAFilesTests(unittest.TestCase):
    def read(self, relative: str) -> str:
        return (ROOT / relative).read_text(encoding="utf-8")

    def test_linux_mozconfig_is_full_veil_browser_build(self):
        text = self.read("gecko/mozconfig.linux")
        self.assertIn("ac_add_options --enable-application=browser", text)
        self.assertIn("ac_add_options --with-branding=browser/branding/veil", text)
        self.assertIn("ac_add_options --with-distribution-id=app.veil", text)
        self.assertNotIn("--enable-artifact-builds", text)
        self.assertNotIn("browser/branding/official", text)
        self.assertNotIn("browser/branding/unofficial", text)

    def test_windows_mozconfig_is_full_veil_browser_build(self):
        text = self.read("gecko/mozconfig.windows")
        self.assertIn("ac_add_options --enable-application=browser", text)
        self.assertIn("ac_add_options --with-branding=browser/branding/veil", text)
        self.assertIn("ac_add_options --with-distribution-id=app.veil", text)
        self.assertNotIn("--enable-artifact-builds", text)
        self.assertNotIn("browser/branding/official", text)
        self.assertNotIn("browser/branding/unofficial", text)

    def test_bootstrap_scripts_use_pinned_official_upstream_and_veil_branding(self):
        for relative in ("scripts/gecko-bootstrap.sh", "scripts/gecko-bootstrap.ps1"):
            with self.subTest(relative=relative):
                text = self.read(relative)
                self.assertIn("https://github.com/mozilla-firefox/firefox.git", text)
                self.assertIn("REVISION", text)
                self.assertIn("gecko_branding.py", text)
                self.assertNotIn("checkout main", text)
                self.assertNotIn("checkout origin/main", text)

    def test_build_scripts_run_full_bootstrap_build_and_package(self):
        shell = self.read("scripts/gecko-build.sh")
        self.assertIn('--application-choice="Firefox for Desktop"', shell)
        self.assertIn("mach build", shell)
        self.assertIn("mach package", shell)

        powershell = self.read("scripts/gecko-build.ps1")
        self.assertIn('Firefox for Desktop', powershell)
        self.assertIn('"build"', powershell)
        self.assertIn('"package"', powershell)

    def test_verifier_wrappers_call_shared_manifest_verifier(self):
        shell = self.read("scripts/gecko-verify.sh")
        powershell = self.read("scripts/gecko-verify.ps1")
        self.assertIn("tools/gecko_manifest.py", shell)
        self.assertIn("tools/gecko_manifest.py", powershell.replace("\\", "/"))

    def test_ci_has_fast_checks_and_explicit_full_build_jobs(self):
        workflow = self.read(".github/workflows/gecko-stage-a.yml")
        self.assertIn("workflow_dispatch:", workflow)
        self.assertIn("branches: [main]", workflow)
        self.assertIn("verify:", workflow)
        self.assertIn("linux-full-build:", workflow)
        self.assertIn("windows-full-build:", workflow)
        self.assertIn("full_build", workflow)
        self.assertIn("gecko/REVISION", workflow)
        self.assertIn("MozillaBuild", workflow)
        self.assertIn("unittest discover -s tests -p 'test_*.py' -v", workflow)
        self.assertIn("tools/gecko_prefs.py", workflow)


if __name__ == "__main__":
    unittest.main()
