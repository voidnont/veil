#!/usr/bin/env python3

from __future__ import annotations

import argparse
import re
import subprocess
from collections import Counter
from pathlib import Path


class ManifestError(RuntimeError):
    """Raised when the Veil Gecko source manifest is invalid."""


_SHA_RE = re.compile(r"^[0-9a-f]{40}$")


def read_revision(repo_root: Path | str) -> str:
    root = Path(repo_root)
    path = root / "gecko" / "REVISION"
    try:
        lines = [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    except FileNotFoundError as exc:
        raise ManifestError(f"missing Gecko revision file: {path}") from exc

    if len(lines) != 1:
        raise ManifestError("gecko/REVISION must contain exactly one non-empty line")

    revision = lines[0]
    if not _SHA_RE.fullmatch(revision):
        raise ManifestError("gecko/REVISION must be a 40-character lowercase hexadecimal Git SHA")
    return revision


def read_patch_series(repo_root: Path | str) -> list[str]:
    root = Path(repo_root)
    path = root / "gecko" / "patches" / "series"
    try:
        raw_lines = path.read_text(encoding="utf-8").splitlines()
    except FileNotFoundError as exc:
        raise ManifestError(f"missing Gecko patch series: {path}") from exc

    entries: list[str] = []
    for raw in raw_lines:
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        entries.append(line)
    return entries


def verify_manifest(repo_root: Path | str) -> None:
    root = Path(repo_root)
    read_revision(root)
    entries = read_patch_series(root)

    duplicates = sorted(name for name, count in Counter(entries).items() if count > 1)
    if duplicates:
        raise ManifestError(f"duplicate patch entry in gecko/patches/series: {duplicates[0]}")

    patch_root = root / "gecko" / "patches"
    listed = set(entries)
    for name in entries:
        patch_path = patch_root / name
        if not patch_path.is_file():
            raise ManifestError(f"listed patch does not exist: {name}")

    actual = {path.name for path in patch_root.glob("*.patch") if path.is_file()}
    unlisted = sorted(actual - listed)
    if unlisted:
        raise ManifestError(f"unlisted patch file in gecko/patches: {unlisted[0]}")


def _git(source_dir: Path, *args: str) -> str:
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=source_dir,
            check=True,
            capture_output=True,
            text=True,
        )
    except (FileNotFoundError, subprocess.CalledProcessError) as exc:
        raise ManifestError(f"failed to inspect Gecko checkout at {source_dir}") from exc
    return result.stdout.strip()


def verify_checkout(
    repo_root: Path | str,
    source_dir: Path | str,
    *,
    require_clean: bool = True,
) -> None:
    root = Path(repo_root)
    source = Path(source_dir)
    verify_manifest(root)
    expected = read_revision(root)
    actual = _git(source, "rev-parse", "HEAD")
    if actual != expected:
        raise ManifestError(f"checkout HEAD {actual} does not match gecko/REVISION {expected}")

    if require_clean:
        status = _git(source, "status", "--porcelain")
        if status:
            raise ManifestError("Gecko checkout is not clean before Veil patches/configuration are applied")


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify Veil's pinned Gecko source manifest and checkout")
    parser.add_argument("--repo-root", default=".", help="Veil repository root")
    parser.add_argument("--source", help="Gecko source checkout directory")
    parser.add_argument("--manifest-only", action="store_true", help="Validate only gecko/REVISION and patches/series")
    parser.add_argument("--allow-dirty", action="store_true", help="Do not require a clean Gecko checkout")
    args = parser.parse_args()

    try:
        if args.manifest_only:
            verify_manifest(args.repo_root)
        else:
            if not args.source:
                parser.error("--source is required unless --manifest-only is used")
            verify_checkout(args.repo_root, args.source, require_clean=not args.allow_dirty)
    except ManifestError as exc:
        parser.exit(1, f"error: {exc}\n")

    print("Gecko manifest verification passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
