# Veil Browser 0.9.0 — Stage C Privacy Design

Date: 2026-09-15
Status: Approved for implementation through the user's standing approval of recommended Veil work and explicit selection of Stage C.

## Goal

Add an explicit, test-covered Veil privacy baseline across desktop Gecko, Android GeckoView, and the native iOS WebKit shell without changing web standards behavior or carrying engine-internal patches.

## Product principle

Veil's default privacy posture should reduce product telemetry and tracking exposure while preserving normal site compatibility. Privacy settings must stay at supported product/runtime boundaries. Veil will not disable JavaScript, WebRTC, cookies, WebGL, media APIs, storage APIs, DRM, or other ordinary web capabilities simply for branding.

## Approaches considered

1. **Conservative privacy layer — selected.** Disable product telemetry/experiments, enable Global Privacy Control, keep standard enhanced tracking protection, and disable developer exposure by default. Lowest long-term maintenance risk and best compatibility.
2. Minimal upstream defaults. Maximum compatibility but gives Veil little explicit privacy policy of its own.
3. Aggressive hardening. Disable or heavily restrict cookies, WebRTC, APIs, storage, and fingerprintable features. Stronger anti-tracking posture but unacceptable site-breakage risk for 0.9.0.

## Desktop Gecko: Windows, Linux, macOS

Desktop builds use a Veil-owned preference file at `gecko/prefs/veil.js`. The bootstrap layer copies this into the already-supported Gecko branding preference hook at `browser/branding/veil/pref/firefox-branding.js` after the unofficial upstream branding tree has been copied, preserving upstream branding defaults and appending Veil defaults.

Veil defaults:

```js
pref("datareporting.healthreport.uploadEnabled", false);
pref("datareporting.policy.dataSubmissionEnabled", false);
pref("toolkit.telemetry.enabled", false);
pref("toolkit.telemetry.unified", false);
pref("browser.newtabpage.activity-stream.telemetry", false);
pref("browser.newtabpage.activity-stream.feeds.telemetry", false);
pref("app.shield.optoutstudies.enabled", false);
pref("browser.discovery.enabled", false);
pref("privacy.globalprivacycontrol.enabled", true);
pref("privacy.trackingprotection.enabled", true);
pref("privacy.trackingprotection.pbmode.enabled", true);
```

No Stage C preference may disable JavaScript, WebRTC, WebGL, site storage, media playback, service workers, extensions, password storage, Safe Browsing, or application updates. Stage C also does not force a nonstandard cookie policy or `privacy.resistFingerprinting`; those have materially larger compatibility consequences and require a separate product decision.

## Android GeckoView

Veil will build its `GeckoRuntime` from explicit `GeckoRuntimeSettings` rather than `GeckoRuntime.create(this)` defaults. The runtime will:

- enable Global Privacy Control;
- use GeckoView's default Enhanced Tracking Protection level rather than a custom filtering policy;
- keep Safe Browsing enabled at GeckoView's default level;
- disable remote debugging by default;
- disable web-console forwarding and Gecko debug logging by default.

The implementation uses public GeckoView APIs from the pinned Gecko revision. It does not alter GeckoView engine internals.

## iOS WebKit

Apple platform policy requires Veil's iOS shell to use WebKit. Stage C keeps normal persistent browsing through `WKWebsiteDataStore.default()` so users can remain signed in and ordinary web storage works.

A dedicated privacy configuration helper will:

- explicitly use the default persistent website data store;
- keep content JavaScript enabled for normal web compatibility;
- prevent JavaScript from opening windows automatically unless initiated through supported browser behavior;
- keep the `WKWebView` non-inspectable in production/default operation on iOS versions that expose `isInspectable`.

Veil will not pretend to provide Gecko's tracking-protection controls on iOS and will not inject fake `navigator.globalPrivacyControl` JavaScript. WebKit privacy behavior remains WebKit-native.

## Testing

New source-level privacy contract tests will verify:

- every required desktop pref is present with the intended value;
- dangerous compatibility-breaking prefs are absent from the Veil overlay;
- the desktop bootstrap invokes the preference overlay step;
- Android runtime creation explicitly enables GPC and disables debugging/log forwarding while using default ETP;
- iOS uses the dedicated privacy configuration, persistent storage, normal JavaScript, popup restriction, and non-inspectability;
- the existing Gecko manifest, branding, platform-target, Android, iOS, and desktop build checks continue to pass.

Full builds remain the final validation gate. Desktop builds must still package successfully from the exact pinned Gecko SHA, Android must produce the local GeckoView artifact and Veil APK, and iOS must compile for the simulator.

## Acceptance criteria

Stage C is complete when:

- desktop Veil builds consume the exact Veil preference overlay above;
- Android uses explicit GeckoView privacy/runtime settings without custom engine patches;
- iOS uses explicit WebKit privacy configuration without sacrificing ordinary persistent browsing;
- privacy contract tests pass on Linux and Windows CI;
- no Stage C setting intentionally changes standards semantics or disables major web-platform capabilities;
- all platform builds that Stage C touches compile successfully.
