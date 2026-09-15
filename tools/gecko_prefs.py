#!/usr/bin/env python3

from __future__ import annotations

import argparse
from pathlib import Path


class PreferenceError(RuntimeError):
    """Raised when Veil's Gecko preference overlay cannot be applied."""


MARKER = "// Veil Stage C product privacy defaults"


def apply_preferences(repo_root: Path | str, source_dir: Path | str) -> Path:
    repo = Path(repo_root)
    source = Path(source_dir)
    veil_prefs = repo / "gecko" / "prefs" / "veil.js"
    destination = source / "browser" / "branding" / "veil" / "pref" / "firefox-branding.js"

    if not veil_prefs.is_file():
        raise PreferenceError(f"Veil preference file not found: {veil_prefs}")
    if not destination.is_file():
        raise PreferenceError(f"Gecko branding preference file not found: {destination}")

    text = destination.read_text(encoding="utf-8")
    if MARKER not in text:
        prefs = veil_prefs.read_text(encoding="utf-8").strip()
        destination.write_text(
            text.rstrip() + "\n\n" + MARKER + "\n" + prefs + "\n",
            encoding="utf-8",
        )
    return destination


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Apply Veil privacy preferences to a prepared Gecko checkout"
    )
    parser.add_argument("--repo-root", default=".", help="Veil repository root")
    parser.add_argument("--source", required=True, help="Prepared Gecko source checkout")
    args = parser.parse_args()

    try:
        destination = apply_preferences(args.repo_root, args.source)
    except PreferenceError as exc:
        parser.exit(1, f"error: {exc}\n")

    print(f"Veil Gecko privacy preferences prepared at {destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
