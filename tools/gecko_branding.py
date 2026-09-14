#!/usr/bin/env python3

from __future__ import annotations

import argparse
import shutil
from pathlib import Path


class BrandingError(RuntimeError):
    """Raised when Veil's Gecko branding overlay is incomplete or invalid."""


IDENTITY_FILES = (
    Path("configure.sh"),
    Path("locales/en-US/brand.ftl"),
    Path("locales/en-US/brand.properties"),
)
FORBIDDEN_BRAND_TERMS = ("Firefox", "Nightly", "Mozilla")
ICON_PATTERNS = ("default*.png", "VisualElements_*.png", "PrivateBrowsing_*.png")


def overlay_root(repo_root: Path | str) -> Path:
    return Path(repo_root) / "gecko" / "branding"


def validate_overlay(repo_root: Path | str) -> None:
    root = overlay_root(repo_root)
    for relative in IDENTITY_FILES:
        path = root / relative
        if not path.is_file():
            raise BrandingError(f"missing Veil branding file: {path}")
        text = path.read_text(encoding="utf-8")
        for term in FORBIDDEN_BRAND_TERMS:
            if term in text:
                raise BrandingError(f"forbidden upstream brand term {term!r} in {relative}")

    configure = (root / "configure.sh").read_text(encoding="utf-8")
    for assignment in (
        "MOZ_APP_DISPLAYNAME=Veil",
        "MOZ_APP_BASENAME=Veil",
        "MOZ_APP_VENDOR=Veil",
        "MOZ_MACBUNDLE_ID=app.veil.browser",
    ):
        if assignment not in configure:
            raise BrandingError(f"missing Veil identity assignment: {assignment}")

    fluent = (root / "locales/en-US/brand.ftl").read_text(encoding="utf-8")
    for line in (
        "-brand-short-name = Veil",
        "-brand-full-name = Veil Browser",
        "-brand-product-name = Veil",
        "-vendor-short-name = Veil",
    ):
        if line not in fluent:
            raise BrandingError(f"missing Veil brand string: {line}")


def apply_branding(repo_root: Path | str, source_dir: Path | str) -> Path:
    repo = Path(repo_root)
    source = Path(source_dir)
    validate_overlay(repo)

    upstream = source / "browser" / "branding" / "unofficial"
    destination = source / "browser" / "branding" / "veil"
    if not upstream.is_dir():
        raise BrandingError(f"upstream unofficial branding directory not found: {upstream}")

    if destination.exists():
        shutil.rmtree(destination)
    shutil.copytree(upstream, destination)
    shutil.copytree(overlay_root(repo), destination, dirs_exist_ok=True)

    veil_icon = repo / "assets" / "veil-glass-icon.png"
    if not veil_icon.is_file():
        raise BrandingError(f"Veil product icon not found: {veil_icon}")

    replaced = 0
    for pattern in ICON_PATTERNS:
        for icon_path in destination.glob(pattern):
            if icon_path.is_file():
                shutil.copy2(veil_icon, icon_path)
                replaced += 1

    if replaced == 0:
        raise BrandingError("upstream branding contained no expected PNG product icon targets")

    for relative in IDENTITY_FILES:
        result = destination / relative
        text = result.read_text(encoding="utf-8")
        for term in FORBIDDEN_BRAND_TERMS:
            if term in text:
                raise BrandingError(f"forbidden upstream brand term {term!r} remains in {relative}")

    return destination


def main() -> int:
    parser = argparse.ArgumentParser(description="Create Veil branding inside a pinned Gecko checkout")
    parser.add_argument("--repo-root", default=".", help="Veil repository root")
    parser.add_argument("--source", required=True, help="Pinned Gecko source checkout")
    args = parser.parse_args()

    try:
        destination = apply_branding(args.repo_root, args.source)
    except BrandingError as exc:
        parser.exit(1, f"error: {exc}\n")

    print(f"Veil Gecko branding prepared at {destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
