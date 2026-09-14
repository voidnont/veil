# Veil 0.9.0 Gecko Stage A Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish the reproducible, pinned upstream Gecko build foundation for Veil 0.9.0 without changing the shipped 0.8.9 browser path.

**Architecture:** Veil keeps the existing Rust browser untouched while adding a separate `gecko/` manifest, cross-platform bootstrap/build/verification scripts, and a dedicated CI workflow. The source checkout is external to this repository, is always detached at the exact SHA in `gecko/REVISION`, and is checked before any build. Full Gecko builds are explicit CI jobs because Mozilla documents a large disk/time requirement; Stage A is not complete until both Windows x64 and Linux full-build jobs have succeeded.

**Tech Stack:** Git, Python 3 standard library, Bash, PowerShell, Mozilla `mach`, MozillaBuild on Windows, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-13-gecko-migration-design.md`

## Global Constraints

- Use the actual `mozilla-firefox/firefox` source tree, not a Gecko reimplementation.
- `gecko/REVISION` contains exactly one full immutable upstream commit SHA.
- The initial pinned revision is `9e4ba5f8a056a91000b369dd508c1e438e3a2192`, the upstream `main` HEAD resolved for this implementation on 2026-09-14.
- Release/full builds are non-artifact builds.
- Keep the current Rust/egui 0.8.x browser and installer behavior unchanged during Stage A.
- Do not ship Firefox/Mozilla product branding; Stage A uses upstream unofficial branding only.
- Windows target is x64.
- A successful Windows and Linux full build is required before declaring Stage A complete.

---

### Task 1: Pin and validate the upstream source manifest

**Files:**
- Create: `gecko/REVISION`
- Create: `gecko/README.md`
- Create: `gecko/patches/series`
- Create: `tools/gecko_manifest.py`
- Create: `tests/test_gecko_manifest.py`

**Interfaces:**
- Consumes: repository root and optional Gecko checkout path.
- Produces: `read_revision(repo_root) -> str`, `read_patch_series(repo_root) -> list[str]`, `verify_manifest(repo_root) -> None`, and `verify_checkout(repo_root, source_dir) -> None`.

- [ ] **Step 1: Write failing manifest tests**

Use Python `unittest` cases that prove: the checked-in revision is one lowercase 40-character hex SHA; blank/comment patch-series lines are ignored; duplicate/missing/unlisted patch entries fail; a checkout whose `HEAD` differs from the manifest fails.

- [ ] **Step 2: Run tests and confirm failure**

Run: `python -m unittest tests.test_gecko_manifest -v`
Expected: FAIL because `tools.gecko_manifest` does not yet exist.

- [ ] **Step 3: Add the revision/patch manifest and minimal verifier**

`gecko/REVISION` must contain exactly:

```text
9e4ba5f8a056a91000b369dd508c1e438e3a2192
```

`gecko/patches/series` starts empty. `tools/gecko_manifest.py` validates the revision, patch list, patch file coverage, exact checkout `HEAD`, and a clean checkout when requested.

- [ ] **Step 4: Run tests and manifest-only verification**

Run:

```bash
python -m unittest tests.test_gecko_manifest -v
python tools/gecko_manifest.py --repo-root . --manifest-only
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add gecko tools/gecko_manifest.py tests/test_gecko_manifest.py
git commit -m "Add pinned Gecko source manifest"
```

### Task 2: Add cross-platform verification, bootstrap, and build entrypoints

**Files:**
- Create: `scripts/gecko-verify.sh`
- Create: `scripts/gecko-verify.ps1`
- Create: `scripts/gecko-bootstrap.sh`
- Create: `scripts/gecko-bootstrap.ps1`
- Create: `scripts/gecko-build.sh`
- Create: `scripts/gecko-build.ps1`

**Interfaces:**
- Consumes: `gecko/REVISION`, `gecko/patches/series`, `tools/gecko_manifest.py`.
- Produces: a deterministic external checkout at `.gecko-src` by default; verification wrappers; build wrappers that run a full non-artifact `mach build` and `mach package`.

- [ ] **Step 1: Add verifier wrappers**

Bash calls `python3 tools/gecko_manifest.py --repo-root <root> --source <source>`. PowerShell resolves `python` and calls the same verifier.

- [ ] **Step 2: Add deterministic bootstrap scripts**

The scripts must initialize the source directory if needed, set origin to `https://github.com/mozilla-firefox/firefox.git`, fetch only the pinned SHA, force a detached checkout of that SHA, clean untracked source files, run the verifier, and apply every patch listed in `gecko/patches/series` in order. They must never fetch/build an unpinned branch tip.

- [ ] **Step 3: Add build scripts**

The scripts select `gecko/mozconfig.linux` or `gecko/mozconfig.windows`, place it at the checkout root as `.mozconfig`, run non-interactive bootstrap for `Firefox for Desktop`, run `mach build`, then `mach package`.

Linux command:

```bash
./mach --no-interactive bootstrap --application-choice="Firefox for Desktop"
./mach build
./mach package
```

Windows command uses `mach.ps1` from PowerShell after MozillaBuild is installed and on `PATH`.

- [ ] **Step 4: Exercise script syntax without downloading Gecko**

Run:

```bash
bash -n scripts/gecko-verify.sh scripts/gecko-bootstrap.sh scripts/gecko-build.sh
python tools/gecko_manifest.py --repo-root . --manifest-only
```

On Windows CI, PowerShell parses/executes `gecko-verify.ps1` in manifest-only mode.

- [ ] **Step 5: Commit**

```bash
git add scripts
git commit -m "Add Gecko bootstrap and build entrypoints"
```

### Task 3: Add explicit full-build mozconfigs

**Files:**
- Create: `gecko/mozconfig.linux`
- Create: `gecko/mozconfig.windows`

**Interfaces:**
- Consumes: Mozilla build system at the pinned revision.
- Produces: optimized, non-artifact Firefox Desktop/Gecko builds using upstream unofficial branding and a dedicated `obj-veil` object directory.

- [ ] **Step 1: Add Linux mozconfig**

```text
mk_add_options MOZ_OBJDIR=@TOPSRCDIR@/obj-veil
mk_add_options AUTOCLOBBER=1
ac_add_options --enable-application=browser
ac_add_options --enable-optimize
ac_add_options --disable-debug
ac_add_options --with-branding=browser/branding/unofficial
```

- [ ] **Step 2: Add Windows mozconfig**

Use the same product/build options and object-directory policy. Do not enable artifact builds.

- [ ] **Step 3: Verify neither mozconfig enables artifact mode**

Run a repository check that fails if either file contains `--enable-artifact-builds`.

- [ ] **Step 4: Commit**

```bash
git add gecko/mozconfig.linux gecko/mozconfig.windows
git commit -m "Configure full Gecko builds"
```

### Task 4: Add Stage A CI with cheap PR checks and explicit full builds

**Files:**
- Create: `.github/workflows/gecko-stage-a.yml`

**Interfaces:**
- Consumes: all Stage A manifest/scripts/mozconfigs.
- Produces: fast validation on PRs/pushes plus manually dispatchable Linux and Windows full-build jobs; each full-build artifact records the exact Gecko revision.

- [ ] **Step 1: Add fast validation job**

On relevant PRs and pushes, checkout the Veil branch, run Python unit tests, run manifest-only verification, syntax-check Bash scripts on Linux, and check PowerShell verifier parsing on Windows.

- [ ] **Step 2: Add Linux full-build job**

On `workflow_dispatch` with `full_build=true`, run `scripts/gecko-bootstrap.sh` then `scripts/gecko-build.sh`, verify `git -C .gecko-src rev-parse HEAD` equals `gecko/REVISION`, and upload a small `gecko-build-metadata-linux` artifact containing the SHA and build log tail. The full packaged runtime remains a CI artifact only when runner capacity permits it.

- [ ] **Step 3: Add Windows x64 full-build job**

Install current MozillaBuild via Chocolatey, add `C:\mozilla-build\bin` to `PATH`, run the PowerShell bootstrap/build scripts, verify exact `HEAD`, and upload `gecko-build-metadata-windows` plus the produced package when runner capacity permits.

- [ ] **Step 4: Keep Stage A completion honest**

The workflow and docs must state that configuration alone does not satisfy Stage A: both full-build jobs must complete successfully at least once for the pinned SHA before Stage A can be marked complete.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/gecko-stage-a.yml
git commit -m "Add Gecko Stage A CI"
```

### Task 5: Review and hand off for CI verification

**Files:**
- Modify if needed: Stage A files above only.

**Interfaces:**
- Consumes: completed Stage A branch.
- Produces: a reviewable PR that leaves `main` and the 0.8.9 release untouched.

- [ ] **Step 1: Run all fast checks**

Run:

```bash
python -m unittest tests.test_gecko_manifest -v
python tools/gecko_manifest.py --repo-root . --manifest-only
bash -n scripts/gecko-verify.sh scripts/gecko-bootstrap.sh scripts/gecko-build.sh
```

- [ ] **Step 2: Review branch diff for scope**

Confirm no existing Rust source, 0.8.9 installer, or release workflow was modified.

- [ ] **Step 3: Open a PR**

PR title: `Start Veil 0.9.0 Gecko Stage A`

The PR body must explicitly say that Stage A remains pending until Windows x64 and Linux full-build jobs succeed for revision `9e4ba5f8a056a91000b369dd508c1e438e3a2192`.
