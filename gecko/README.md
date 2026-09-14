# Veil Gecko source manifest

Veil 0.9.0 is migrating to Mozilla's actual Gecko browser engine. The Gecko source tree is intentionally **not vendored** into this repository.

`REVISION` contains the exact upstream `mozilla-firefox/firefox` commit that Veil builds. Build and CI scripts must fetch and checkout that exact commit; they must not build a moving branch tip.

Current Stage A pin:

```text
9e4ba5f8a056a91000b369dd508c1e438e3a2192
```

Upstream source: `https://github.com/mozilla-firefox/firefox`

## Patch series

`patches/series` lists Veil-owned Gecko patches in application order. It is empty during the initial Stage A build. Every `*.patch` file in `patches/` must be listed exactly once.

## Local flow

Linux:

```bash
./scripts/gecko-bootstrap.sh
./scripts/gecko-build.sh
```

Windows (PowerShell, with MozillaBuild installed):

```powershell
./scripts/gecko-bootstrap.ps1
./scripts/gecko-build.ps1
```

The default external source directory is `.gecko-src` and can be overridden by the scripts' source-directory option.

## Stage A status

The presence of these files does **not** by itself complete Stage A. Stage A is complete only after a full Windows x64 build and a full Linux build both succeed for the exact SHA in `REVISION` and CI records that SHA.

The existing Veil 0.8.9 Rust/egui browser remains untouched while this migration foundation is built.
