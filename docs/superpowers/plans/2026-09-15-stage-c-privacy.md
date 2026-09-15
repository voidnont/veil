# Veil Stage C Privacy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add explicit, test-covered Veil privacy defaults across desktop Gecko, Android GeckoView, and iOS WebKit without changing ordinary web standards behavior.

**Architecture:** Keep privacy changes at supported product/runtime boundaries. Desktop Gecko appends a Veil pref file through the existing branding-pref hook; Android constructs GeckoRuntime with public GeckoView privacy/runtime settings; iOS creates WKWebView through a dedicated privacy configuration helper.

**Tech Stack:** Python unittest, Mozilla Gecko/Firefox prefs, GeckoView Java APIs, SwiftUI/WebKit, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-15-stage-c-privacy-design.md`

## Global Constraints

- Work directly on `main` because the user explicitly requested a main-only workflow.
- Do not add engine-internal Gecko patches for Stage C.
- Do not disable JavaScript, WebRTC, WebGL, site storage, media, service workers, Safe Browsing, or application updates.
- Keep the exact Gecko revision pinned by `gecko/REVISION`.
- Android must use public GeckoView APIs from the pinned revision.
- iOS remains WebKit-native and uses persistent normal browsing data.

---

### Task 1: Desktop Gecko privacy overlay

**Files:**
- Create: `gecko/prefs/veil.js`
- Create: `tools/gecko_prefs.py`
- Create: `tests/test_gecko_stage_c_privacy.py`
- Modify: `scripts/gecko-bootstrap.sh`
- Modify: `scripts/gecko-bootstrap.ps1`
- Modify: `.github/workflows/gecko-stage-a.yml`

**Interfaces:**
- Produces: `apply_preferences(repo_root: Path | str, source_dir: Path | str) -> Path` in `tools/gecko_prefs.py`.
- Consumes: prepared `browser/branding/veil/pref/firefox-branding.js` created by Stage B branding.

- [ ] **Step 1: Write failing privacy contract tests**

```python
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
```

Tests verify every required pref exists, forbidden compatibility-breaking prefs are absent, `apply_preferences` preserves existing branding prefs while appending Veil prefs once, and both bootstrap scripts invoke `tools/gecko_prefs.py` after branding.

- [ ] **Step 2: Run the tests and confirm they fail before implementation**

Run:

```bash
python -m unittest tests.test_gecko_stage_c_privacy -v
```

Expected: failure because `gecko/prefs/veil.js` and `tools/gecko_prefs.py` do not exist yet.

- [ ] **Step 3: Implement the Veil pref file**

Create `gecko/prefs/veil.js` containing exactly the Stage C pref set from the spec, with comments explaining that compatibility-sensitive web capabilities are intentionally left at upstream defaults.

- [ ] **Step 4: Implement preference application**

`tools/gecko_prefs.py` must:

```python
def apply_preferences(repo_root: Path | str, source_dir: Path | str) -> Path:
    repo = Path(repo_root)
    source = Path(source_dir)
    veil_prefs = repo / "gecko" / "prefs" / "veil.js"
    destination = source / "browser" / "branding" / "veil" / "pref" / "firefox-branding.js"
    if not veil_prefs.is_file():
        raise PreferenceError(f"Veil preference file not found: {veil_prefs}")
    if not destination.is_file():
        raise PreferenceError(f"Gecko branding preference file not found: {destination}")
    marker = "// Veil Stage C product privacy defaults"
    text = destination.read_text(encoding="utf-8")
    if marker not in text:
        destination.write_text(text.rstrip() + "\n\n" + marker + "\n" + veil_prefs.read_text(encoding="utf-8").strip() + "\n", encoding="utf-8")
    return destination
```

Provide a CLI with `--repo-root` and required `--source`, mirroring `tools/gecko_branding.py` error handling.

- [ ] **Step 5: Wire bootstrap and CI**

Both bootstrap scripts call `gecko_prefs.py` immediately after `gecko_branding.py`. Add `tests.test_gecko_stage_c_privacy` to Linux/Windows fast verification and add `tools/gecko_prefs.py`/`gecko/prefs/**` to workflow path filters.

- [ ] **Step 6: Run desktop privacy tests**

Run:

```bash
python -m unittest tests.test_gecko_manifest tests.test_gecko_stage_a_files tests.test_gecko_stage_b_identity tests.test_gecko_stage_c_privacy tests.test_platform_targets -v
python tools/gecko_manifest.py --repo-root . --manifest-only
bash -n scripts/gecko-verify.sh scripts/gecko-bootstrap.sh scripts/gecko-build.sh scripts/gecko-build-android.sh
```

Expected: all pass.

### Task 2: Android GeckoView privacy runtime

**Files:**
- Modify: `mobile/android/app/src/main/java/app/veil/browser/MainActivity.java`
- Extend: `tests/test_gecko_stage_c_privacy.py`

**Interfaces:**
- Consumes: `GeckoRuntimeSettings.Builder` and `ContentBlocking.Settings.Builder` from the pinned GeckoView API.

- [ ] **Step 1: Add failing Android source contract test**

Require `MainActivity.java` to contain:

```text
GeckoRuntimeSettings.Builder
.globalPrivacyControlEnabled(true)
.remoteDebuggingEnabled(false)
.consoleOutput(false)
.debugLogging(false)
ContentBlocking.Settings.Builder
.safeBrowsing(ContentBlocking.SafeBrowsing.DEFAULT)
.enhancedTrackingProtectionLevel(ContentBlocking.EtpLevel.DEFAULT)
```

- [ ] **Step 2: Run the test and confirm failure**

Run:

```bash
python -m unittest tests.test_gecko_stage_c_privacy.GeckoStageCPrivacyTests.test_android_runtime_privacy_defaults -v
```

Expected: fail because `MainActivity` currently calls `GeckoRuntime.create(this)` without explicit settings.

- [ ] **Step 3: Implement explicit runtime settings**

Replace default runtime creation with:

```java
GeckoRuntimeSettings runtimeSettings = new GeckoRuntimeSettings.Builder()
        .globalPrivacyControlEnabled(true)
        .remoteDebuggingEnabled(false)
        .consoleOutput(false)
        .debugLogging(false)
        .contentBlocking(new ContentBlocking.Settings.Builder()
                .safeBrowsing(ContentBlocking.SafeBrowsing.DEFAULT)
                .enhancedTrackingProtectionLevel(ContentBlocking.EtpLevel.DEFAULT)
                .build())
        .build();
runtime = GeckoRuntime.create(this, runtimeSettings);
```

Add imports for `ContentBlocking` and `GeckoRuntimeSettings`.

- [ ] **Step 4: Run privacy and platform tests**

Run:

```bash
python -m unittest tests.test_gecko_stage_c_privacy tests.test_platform_targets -v
```

Expected: pass.

### Task 3: iOS WebKit privacy configuration

**Files:**
- Create: `mobile/ios/Sources/PrivacyConfiguration.swift`
- Modify: `mobile/ios/Sources/BrowserView.swift`
- Extend: `tests/test_gecko_stage_c_privacy.py`

**Interfaces:**
- Produces: `PrivacyConfiguration.makeWebViewConfiguration() -> WKWebViewConfiguration` and `PrivacyConfiguration.apply(to: WKWebView)`.

- [ ] **Step 1: Add failing iOS privacy contract test**

Require the helper to include:

```swift
configuration.websiteDataStore = .default()
configuration.defaultWebpagePreferences.allowsContentJavaScript = true
configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
webView.isInspectable = false
```

Require `BrowserView.swift` to use `PrivacyConfiguration.makeWebViewConfiguration()` and `PrivacyConfiguration.apply(to: webView)`.

- [ ] **Step 2: Run the test and confirm failure**

Run:

```bash
python -m unittest tests.test_gecko_stage_c_privacy.GeckoStageCPrivacyTests.test_ios_webkit_privacy_defaults -v
```

Expected: fail because the helper does not exist.

- [ ] **Step 3: Implement helper and integrate it**

```swift
import WebKit

enum PrivacyConfiguration {
    static func makeWebViewConfiguration() -> WKWebViewConfiguration {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .default()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = true
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        return configuration
    }

    static func apply(to webView: WKWebView) {
        if #available(iOS 16.4, *) {
            webView.isInspectable = false
        }
    }
}
```

Create the `WKWebView` from the helper in `BrowserModel.init()` and apply runtime privacy after construction.

- [ ] **Step 4: Run privacy tests and iOS build CI**

Run source contracts locally, then rely on the existing `Build Veil for iOS Simulator` GitHub Actions job as the compile gate.

### Task 4: Cross-platform verification

**Files:**
- No new production files.
- Verify existing workflows and resulting artifacts.

- [ ] **Step 1: Run all fast tests**

```bash
python -m unittest tests.test_gecko_manifest tests.test_gecko_stage_a_files tests.test_gecko_stage_b_identity tests.test_gecko_stage_c_privacy tests.test_platform_targets -v
```

- [ ] **Step 2: Confirm GitHub fast CI is green on Linux and Windows**

Both Stage C contract suites, manifest verification, shell syntax, and PowerShell parsing must pass.

- [ ] **Step 3: Confirm mobile compile gates**

The iOS simulator job must succeed. Android must progress through the pinned GeckoView build and produce `mobile/android/app/build/outputs/apk/debug/app-debug.apk` in the full Android job.

- [ ] **Step 4: Trigger a full Gecko validation only after the current full Android run is no longer active**

Use a `[gecko-full]` commit only when needed to avoid intentionally running duplicate multi-hour Gecko jobs concurrently.

- [ ] **Step 5: Record completion accurately**

Do not call Stage C complete until touched platform builds are green. If Android is still compiling, report Stage C implementation complete but Android validation pending.
