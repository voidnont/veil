# Veil 0.9.0 Gecko Stage B Identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the pinned Gecko browser identify and present itself as Veil without changing Gecko web-platform behavior.

**Architecture:** Keep Mozilla's pinned source untouched until bootstrap, then create `browser/branding/veil` by copying the upstream unofficial branding directory and overlaying Veil-owned identity files and assets. Configure both Windows and Linux builds to use that Veil branding directory, while leaving engine/platform diagnostics and Gecko compatibility tokens intact.

**Tech Stack:** Mozilla Gecko/Firefox build system, Python 3, Bash, PowerShell, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-13-gecko-migration-design.md`

## Global Constraints

- Use the real pinned Gecko source from `gecko/REVISION`.
- Do not modify DOM, CSS/layout, JavaScript semantics, networking, WebRender, or other web-platform behavior.
- The browser product name is `Veil` / `Veil Browser`.
- Do not ship Firefox or Mozilla product branding or claim Mozilla endorsement.
- Keep Gecko platform/version diagnostics intact.
- Keep Veil's maintained delta at the product/branding boundary.
- Work lands directly on `main` per project workflow.

---

### Task 1: Add Veil branding overlay metadata

**Files:**
- Create: `gecko/branding/configure.sh`
- Create: `gecko/branding/locales/en-US/brand.ftl`
- Create: `gecko/branding/locales/en-US/brand.properties`
- Create: `tests/test_gecko_stage_b_identity.py`

**Interfaces:**
- Consumes: upstream `browser/branding/unofficial` copied during bootstrap.
- Produces: Veil display name, vendor string, shortcut name, and full product name.

- [ ] Write tests that require `Veil`, `Veil Browser`, a non-Mozilla vendor, and forbid `Firefox`, `Nightly`, and `Mozilla` in Veil brand strings.
- [ ] Confirm the tests fail before the overlay files exist.
- [ ] Add minimal Veil branding metadata and strings.
- [ ] Run the Stage B tests to green.

### Task 2: Apply branding overlay during bootstrap

**Files:**
- Create: `tools/gecko_branding.py`
- Modify: `scripts/gecko-bootstrap.sh`
- Modify: `scripts/gecko-bootstrap.ps1`
- Modify: `tests/test_gecko_stage_b_identity.py`

**Interfaces:**
- Consumes: pinned Gecko checkout and `gecko/branding` overlay.
- Produces: `browser/branding/veil` inside the external Gecko checkout.

- [ ] Add tests for overlay destination, replacement semantics, and forbidden brand strings.
- [ ] Implement a Python overlay helper that copies upstream unofficial branding to `browser/branding/veil`, overlays Veil files, and validates the result.
- [ ] Call that helper from both bootstrap scripts after revision verification/patch application.
- [ ] Run Stage A + Stage B tests to green.

### Task 3: Switch full builds to Veil branding

**Files:**
- Modify: `gecko/mozconfig.linux`
- Modify: `gecko/mozconfig.windows`
- Modify: `tests/test_gecko_stage_a_files.py`
- Modify: `.github/workflows/gecko-stage-a.yml`

**Interfaces:**
- Consumes: generated `browser/branding/veil` source directory.
- Produces: Windows/Linux Gecko builds configured with `--with-branding=browser/branding/veil`.

- [ ] Change tests to require the Veil branding path and reject official/unofficial branding paths.
- [ ] Update both mozconfigs.
- [ ] Include Stage B identity tests in Linux and Windows fast CI verification.
- [ ] Verify no existing Rust 0.8.9 runtime/release workflow is altered.

### Task 4: Verify on main and choose next step

**Files:**
- Stage B files above only.

**Interfaces:**
- Produces: fast CI evidence for Veil identity overlay on `main`.

- [ ] Run the full fast Python test set and manifest verification in CI.
- [ ] Confirm Linux Bash syntax and Windows PowerShell parsing pass.
- [ ] Confirm `main` contains the Stage B commits.
- [ ] Present next-step choices and automatically continue with the recommended option.
