#!/usr/bin/env python3
import argparse
import hashlib
import xml.etree.ElementTree as ET
from pathlib import Path

WIX_NS = "http://wixtoolset.org/schemas/v4/wxs"
ET.register_namespace("", WIX_NS)


def q(tag: str) -> str:
    return f"{{{WIX_NS}}}{tag}"


def stable_id(prefix: str, text: str) -> str:
    digest = hashlib.sha1(text.encode("utf-8")).hexdigest()[:20]
    return f"{prefix}_{digest}"


def find_primary_executable(source: Path) -> Path:
    candidates = []
    for path in source.rglob("*.exe"):
        lower = path.name.lower()
        if lower in {"veil.exe", "firefox.exe"}:
            priority = 0 if lower == "veil.exe" else 1
            depth = len(path.relative_to(source).parts)
            candidates.append((priority, depth, str(path).lower(), path))
    if not candidates:
        raise ValueError(f"No Veil/Firefox browser executable found under {source}")
    candidates.sort()
    return candidates[0][-1]


def build_manifest(source: Path, output: Path, version: str = "0.9.0") -> None:
    source = Path(source).resolve()
    output = Path(output)
    if not source.is_dir():
        raise ValueError(f"Runtime directory does not exist: {source}")

    files = sorted(
        (path for path in source.rglob("*") if path.is_file()),
        key=lambda path: str(path.relative_to(source)).lower(),
    )
    if not files:
        raise ValueError(f"Runtime directory is empty: {source}")
    primary = find_primary_executable(source)

    wix = ET.Element(q("Wix"))
    package = ET.SubElement(
        wix,
        q("Package"),
        {
            "Name": "Veil Browser",
            "Manufacturer": "Veil",
            "Version": version,
            "Language": "1033",
            "UpgradeCode": "A5A2C11B-BC89-4E5D-AE7D-913CF8B45189",
            "Scope": "perUser",
        },
    )
    ET.SubElement(
        package,
        q("MajorUpgrade"),
        {"DowngradeErrorMessage": "A newer version of Veil Browser is already installed."},
    )
    ET.SubElement(
        package,
        q("MediaTemplate"),
        {"EmbedCab": "yes", "CompressionLevel": "high"},
    )

    local = ET.SubElement(package, q("StandardDirectory"), {"Id": "LocalAppDataFolder"})
    programs = ET.SubElement(local, q("Directory"), {"Id": "ProgramsDirectory", "Name": "Programs"})
    install = ET.SubElement(
        programs,
        q("Directory"),
        {"Id": "INSTALLFOLDER", "Name": "Veil Browser"},
    )
    ET.SubElement(package, q("StandardDirectory"), {"Id": "ProgramMenuFolder"})
    ET.SubElement(package, q("StandardDirectory"), {"Id": "DesktopFolder"})

    directory_nodes = {Path("."): install}
    directory_ids = {Path("."): "INSTALLFOLDER"}

    def ensure_directory(rel_dir: Path):
        rel_dir = Path(rel_dir)
        if str(rel_dir) in {"", "."}:
            return install, "INSTALLFOLDER"
        if rel_dir in directory_nodes:
            return directory_nodes[rel_dir], directory_ids[rel_dir]
        parent_node, _ = ensure_directory(rel_dir.parent)
        directory_id = stable_id("Dir", rel_dir.as_posix())
        node = ET.SubElement(
            parent_node,
            q("Directory"),
            {"Id": directory_id, "Name": rel_dir.name},
        )
        directory_nodes[rel_dir] = node
        directory_ids[rel_dir] = directory_id
        return node, directory_id

    component_ids = []
    for file_path in files:
        rel = file_path.relative_to(source)
        parent_node, parent_id = ensure_directory(rel.parent)
        component_id = stable_id("Cmp", rel.as_posix())
        file_id = stable_id("Fil", rel.as_posix())
        component = ET.SubElement(
            parent_node,
            q("Component"),
            {"Id": component_id, "Guid": "*"},
        )
        file_element = ET.SubElement(
            component,
            q("File"),
            {
                "Id": file_id,
                "Source": str(file_path),
                "KeyPath": "yes",
            },
        )
        if file_path == primary:
            ET.SubElement(
                file_element,
                q("Shortcut"),
                {
                    "Id": "StartMenuShortcut",
                    "Directory": "ProgramMenuFolder",
                    "Name": "Veil Browser",
                    "WorkingDirectory": parent_id,
                },
            )
            ET.SubElement(
                file_element,
                q("Shortcut"),
                {
                    "Id": "DesktopShortcut",
                    "Directory": "DesktopFolder",
                    "Name": "Veil Browser",
                    "WorkingDirectory": parent_id,
                },
            )
        component_ids.append(component_id)

    feature = ET.SubElement(
        package,
        q("Feature"),
        {"Id": "MainFeature", "Title": "Veil Browser", "Level": "1"},
    )
    for component_id in component_ids:
        ET.SubElement(feature, q("ComponentRef"), {"Id": component_id})

    output.parent.mkdir(parents=True, exist_ok=True)
    tree = ET.ElementTree(wix)
    ET.indent(tree, space="  ")
    tree.write(output, encoding="utf-8", xml_declaration=True)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate a WiX manifest for the complete Veil Gecko runtime."
    )
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--version", default="0.9.0")
    args = parser.parse_args()
    build_manifest(args.source, args.output, args.version)


if __name__ == "__main__":
    main()
