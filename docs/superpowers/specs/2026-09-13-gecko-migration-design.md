# Veil Browser 0.9.0 — Gecko Migration Design

Date: 2026-09-13
Status: Approved architecture; implementation not started

## 1. Goal

Veil 0.9.0 will stop treating Veil Engine as an independent reimplementation of browser behavior and will instead ship Mozilla's actual Gecko browser engine code at a pinned upstream revision.

The requirement "carbon copy of Gecko" is interpreted literally at the engine layer: Veil must build and run the real upstream Gecko implementation rather than reproducing selected Gecko algorithms in Veil's Rust renderer.

Veil remains its own product and branding. It must not claim to be Firefox or an official Mozilla build.

## 2. Architectural decision

Veil will become a downstream Gecko-based browser distribution built from the official Firefox/Gecko source tree, with a deliberately small Veil patch/overlay layer.

The production engine path will therefore be:

```text
Veil product branding / product preferences / packaging
                    |
                    v
        Mozilla browser platform
                    |
                    v
 Gecko DOM + Stylo + SpiderMonkey + Necko + WebRender + media/Web APIs
```

The current Rust/egui browser shell and independent engine remain available only during migration as the 0.8.x legacy path. They are not part of the final 0.9.0 production web-content path.

## 3. Why this approach

Three approaches were considered:

1. Reimplement Gecko behavior inside Veil Engine.
2. Embed Gecko behind Veil's existing egui shell.
3. Build Veil as a downstream of the actual Firefox/Gecko source tree.

Option 3 is selected.

Option 1 cannot guarantee Gecko-equivalent behavior because every DOM, CSS, JS, layout, networking, graphics, media, accessibility and Web API detail would have to be independently recreated and continuously kept in sync.

Option 2 preserves more of the current UI but creates a fragile custom embedding boundary around desktop Gecko and would require Veil to maintain a large native integration layer that Mozilla itself does not expose as a small desktop embedding SDK.

Option 3 minimizes divergence: the engine is real Gecko and Veil's maintained delta stays at the product/branding/configuration boundary whenever possible.

## 4. Upstream source and pinning

The build system will use Mozilla's current Firefox source repository as the upstream source of Gecko:

`https://github.com/mozilla-firefox/firefox`

Veil will not vendor the entire multi-gigabyte Gecko repository into `voidnont/veilbrowser`. Instead, the Veil repository will contain an immutable revision manifest plus a small patch/overlay set.

New files:

```text
gecko/
  REVISION
  README.md
  mozconfig.windows
  mozconfig.linux
  patches/
    series
    *.patch
  branding/
    ...Veil-owned product assets...
  prefs/
    veil.js
scripts/
  gecko-bootstrap.ps1
  gecko-bootstrap.sh
  gecko-build.ps1
  gecko-build.sh
  gecko-verify.ps1
  gecko-verify.sh
LICENSES/
  MPL-2.0.txt
  THIRD-PARTY-NOTICES.md
SOURCE.md
```

`gecko/REVISION` contains exactly one full upstream commit SHA. The first implementation commit will resolve the then-current `mozilla-firefox/firefox` `main` HEAD, record that immutable SHA, and every CI/release build will check out exactly that SHA. No release build may build from an unpinned branch tip.

Upstream updates happen by changing `gecko/REVISION`, rebasing the Veil patch series, and passing the same compatibility/release gates as a normal release.

## 5. Patch policy

The primary rule is: do not fork Gecko internals unless Veil cannot be implemented at the product boundary.

Allowed patch categories:

- Veil product name, application identifiers and version metadata
- Veil-owned icons and visual assets
- default preferences that are genuinely part of Veil's product behavior
- Veil-specific new-tab/home surfaces
- installer/product integration required to ship Veil
- clearly isolated privacy/product hooks that cannot be expressed through supported configuration

Disallowed by default:

- custom DOM behavior
- custom CSS/layout algorithms
- custom JavaScript semantics
- custom networking protocol behavior
- WebRender changes
- modified web-exposed APIs solely to make Veil "different"
- speculative performance changes to Gecko internals

Any engine-internal patch must document why upstream behavior cannot be preserved and must have an engine-compatibility test.

## 6. Branding and trademarks

Veil will use Veil names, icons and product identifiers only.

The distributed application will not use Firefox or Mozilla logos, Firefox product names, or branding that implies Mozilla publishes or endorses Veil.

Technical documentation may accurately state that Veil is built from Mozilla open-source technology and Gecko, accompanied by a clear statement that Veil is not officially associated with Mozilla or its products.

The User-Agent will retain Gecko-compatible engine tokens and engine-version semantics, but the downstream product token will identify Veil rather than claim to be an official Firefox build.

## 7. Licensing model

The current repository is primarily MIT-licensed Veil code. That cannot be used as a blanket license for imported or modified Mozilla files.

The project becomes multi-license:

- original Veil-authored files keep their existing license unless explicitly changed;
- Mozilla-derived source files retain their original license notices and applicable MPL 2.0 terms;
- third-party code inside the upstream source tree keeps its own license terms;
- release packages preserve upstream legal notices and Veil adds its own notices without replacing upstream notices.

Each binary release will include a source-availability notice in `SOURCE.md` identifying:

1. the exact Gecko revision from `gecko/REVISION`;
2. the public upstream source location;
3. the exact Veil patches/overlays used for that release;
4. the Veil source commit used to produce the binary.

The repository will include the MPL 2.0 text and a third-party notice index. This design is an engineering compliance plan, not legal advice; distribution must continue to follow the actual licenses and Mozilla trademark rules.

## 8. Build architecture

### Windows

CI will bootstrap the Mozilla build prerequisites, fetch the pinned Gecko source, apply Veil's patches/overlay, and run Mozilla's `mach` build system.

The build will use a checked-in Veil mozconfig. Release builds will be non-artifact builds because the goal is to ship the pinned Gecko engine itself, not reuse an opaque prebuilt browser engine.

The expected production steps are conceptually:

```text
fetch upstream repository
checkout gecko/REVISION
verify revision
apply gecko/patches/series
apply Veil branding/prefs overlay
./mach build
./mach package
run Veil verification suite
stage complete runtime tree
build MSI
publish source manifest + installer checksums
```

### Linux

Linux CI follows the same pinned revision and patch set. It must at minimum complete a full build and verification run so Veil does not accidentally create Windows-only Gecko patches.

## 9. Packaging

The existing 0.8.x MSI contains two Veil executables. Gecko changes this completely: a Gecko browser is a runtime tree containing many executables, DLLs, resources, localization files and packaged browser assets.

The 0.9.0 MSI therefore installs the complete built Gecko/Veil runtime tree, not just `veil-browser.exe` and `veil-engine.exe`.

Windows packaging requirements:

- x64 MSI for the first 0.9.0 release;
- per-user installation unless a later explicit product decision changes it;
- clean major-upgrade behavior from later Gecko-based Veil releases;
- Start Menu entry and optional desktop shortcut using Veil branding;
- all Gecko runtime dependencies preserved exactly as produced by the build/package stage;
- no Firefox logos or official-channel branding;
- SHA-256 checksum published with every MSI;
- release page includes pinned Gecko revision and Veil source revision.

The existing independent `veil-engine.exe` is not included in the final 0.9.0 MSI once the Gecko migration acceptance gates pass.

## 10. Migration stages

### Stage A — Reproducible Gecko build

Add revision pinning, bootstrap scripts, mozconfigs and CI capable of producing an unbranded/unofficial Gecko build from the pinned source revision.

Acceptance:

- Windows x64 full Gecko build succeeds;
- Linux full Gecko build succeeds;
- CI records the exact Gecko SHA in artifacts;
- no Veil engine behavior is changed yet.

### Stage B — Veil product identity

Apply Veil-owned branding and application identifiers through the smallest possible overlay/patch set.

Acceptance:

- browser identifies itself as Veil in UI/product metadata;
- no Mozilla/Firefox logos ship;
- `about:support`/diagnostic information still shows the expected Gecko platform/version data;
- ordinary websites are rendered by Gecko, not `src/engine.rs`.

### Stage C — Veil product preferences and privacy configuration

Port only Veil product defaults that can be represented safely as preferences or browser-front-end configuration.

Acceptance:

- privacy defaults are explicit and test-covered;
- no changes to standards behavior are introduced merely for privacy branding;
- site compatibility remains Gecko-equivalent for the pinned revision unless a Veil preference intentionally changes a web-visible capability.

### Stage D — Release integration

Package the complete runtime into Veil's Windows MSI and update GitHub release automation.

Acceptance:

- clean install succeeds on a fresh Windows environment;
- launch succeeds from Start Menu and direct executable;
- uninstall removes product files without deleting unrelated user data;
- MSI contains the same build that passed verification;
- checksum and source manifest are attached to the GitHub release.

### Stage E — Legacy-engine retirement

After Gecko-based Veil passes all acceptance gates, stop shipping the Rust web engine.

The old engine source may remain in the repository for one migration release if useful for reference, but it is not linked into or invoked by the production browser. A later cleanup removes dead dependencies such as Boa, the custom DOM/layout renderer and renderer IPC when no longer used by tooling/tests.

## 11. Compatibility and regression gates

A Gecko-based Veil release must prove both "real Gecko is running" and "Veil did not accidentally fork web behavior."

Required gates:

1. Upstream revision verification
   - checked-out source SHA must exactly equal `gecko/REVISION`.

2. Patch audit
   - every applied patch must be listed in `gecko/patches/series`;
   - CI fails on an uncommitted or unlisted Gecko modification.

3. Build verification
   - full Windows release build;
   - full Linux release build.

4. Gecko identity smoke tests
   - diagnostic build/platform metadata matches the pinned revision;
   - Gecko rendering path is used for HTTP/HTTPS documents.

5. Web-platform smoke tests
   - DOM mutation/events;
   - CSS flexbox/grid/positioning;
   - ES modules and Promises;
   - Fetch/XHR;
   - Canvas/SVG;
   - WebSockets;
   - history/navigation;
   - media playback where CI environment permits;
   - local/session storage and IndexedDB;
   - selected upstream WPT/mochitest coverage for each modified engine-adjacent area.

6. Product smoke tests
   - tabs/navigation/downloads;
   - new tab/home;
   - profile creation;
   - settings/preferences;
   - crash-safe restart;
   - private browsing launches and isolates state using upstream Gecko behavior.

7. Installer smoke tests
   - silent install;
   - normal launch;
   - upgrade from prior Gecko-based test build;
   - uninstall;
   - file/version metadata validation.

## 12. Performance policy

Veil will not carry custom Gecko performance patches in 0.9.0 unless a measured regression is caused by Veil's own overlay.

The first goal is equivalence and maintainability. Gecko already contains mature incremental style, layout, compositor, networking, JavaScript and graphics systems. Replacing them with Veil-specific versions would defeat the purpose of this migration.

Performance work after migration should first remove Veil overhead or submit generally useful engine fixes upstream rather than maintaining a permanent private fork.

## 13. Security and update policy

Pinning gives reproducible builds but creates a security responsibility: Veil must not remain indefinitely on an old Gecko revision.

For every upstream security release affecting the pinned code, Veil must either:

- advance `gecko/REVISION` to a fixed upstream revision and rebuild; or
- apply an upstream security patch exactly and record it in the patch series until the revision can advance.

A release is blocked if it knowingly ships a Gecko revision with a publicly fixed critical security issue without an equivalent fix in the Veil patch set.

## 14. Failure and rollback strategy

The 0.8.9 MSI remains the last independent-engine release and is not overwritten.

During migration, incomplete Gecko work may land on `main` only when it does not replace the published 0.8.9 release artifact. The production release pointer changes to 0.9.0 only after all Stage D gates pass.

If a Gecko migration build fails compatibility or installer verification, the fix is made in the migration layer; Veil does not silently fall back to its custom engine inside the same 0.9.0 binary, because that would make behavior unpredictable and violate the "actual Gecko" requirement.

## 15. Definition of done for 0.9.0

Veil 0.9.0 is complete when all of the following are true:

- web content is parsed, styled, scripted, laid out, painted, networked and media-handled by the pinned Mozilla Gecko platform;
- SpiderMonkey is the production JavaScript engine;
- Stylo/Gecko is the production style/layout implementation;
- WebRender/Gecko graphics pipeline is used as produced by upstream;
- the current custom Rust DOM/layout/Boa renderer is not used for production pages;
- Veil's Gecko patch set is small, enumerated and auditable;
- Veil branding is distinct from Mozilla/Firefox branding;
- required licenses/notices and source information ship with the release;
- Windows and Linux builds pass the compatibility gates;
- a Windows x64 MSI installs the complete Gecko-based Veil runtime;
- the GitHub release publishes the MSI, SHA-256 and exact source/revision information.

At that point it is accurate to say: "Veil Browser is built on the actual Mozilla Gecko engine at the exact revision recorded in `gecko/REVISION`, with a small Veil product patch layer." It is not accurate to claim Veil is Firefox or officially associated with Mozilla.

## 16. Authoritative references used for this design

- Mozilla Firefox source/build documentation: https://firefox-source-docs.mozilla.org/setup/windows_build.html
- Mozilla Windows installer build documentation: https://firefox-source-docs.mozilla.org/browser/installer/windows/installer/InstallerBuild.html
- Mozilla Public License 2.0: https://www.mozilla.org/MPL/2.0/
- Mozilla MPL 2.0 FAQ: https://www.mozilla.org/MPL/2.0/FAQ/
- Mozilla trademark/distribution policy: https://www.mozilla.org/foundation/trademarks/distribution-policy/
- Mozilla trademark guidelines: https://www.mozilla.org/foundation/trademarks/policy/
