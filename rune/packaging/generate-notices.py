#!/usr/bin/env python3
"""Produce a deterministic Cargo dependency inventory and full license-text bundle.

Uses Cargo's locked resolve graph; no external Python packages. This inventories sources,
not a proof of the exact code linked into an executable. Release owners review gaps.
"""
import argparse
import hashlib
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
PACKAGING = pathlib.Path(__file__).resolve().parent
MAX_LICENSE_BYTES = 1_000_000


def selected_packages(metadata, roots):
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    selected_roots = [key for key, package in packages.items() if package["name"] in roots]
    missing = set(roots) - {packages[key]["name"] for key in selected_roots}
    if missing:
        raise ValueError("Unknown root packages: " + ", ".join(sorted(missing)))
    seen, pending = set(), list(selected_roots)
    while pending:
        key = pending.pop()
        if key in seen:
            continue
        seen.add(key)
        for dependency in nodes.get(key, {}).get("deps", []):
            # Include normal and build dependencies conservatively; omit dev-only edges.
            if any(kind.get("kind") != "dev" for kind in dependency.get("dep_kinds", [])):
                pending.append(dependency["pkg"])
    return sorted((packages[key] for key in seen), key=lambda p: (p["name"], p["version"], p.get("source") or ""))


def license_files(package):
    directory = pathlib.Path(package["manifest_path"]).parent
    explicit = package.get("license_file")
    paths = []
    if explicit:
        path = pathlib.Path(explicit)
        paths.append(path if path.is_absolute() else directory / path)
    # Workspace crates, notably Rune, can inherit the repository's root license.
    current = directory
    for _ in range(6):
        candidates = []
        for path in current.iterdir():
            name = path.name.upper()
            if path.is_file() and any(marker in name for marker in ("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE", "UNLICENSE")):
                if path.suffix.lower() not in (".rs", ".toml", ".json", ".lock", ".png", ".ttf", ".otf"):
                    candidates.append(path)
        paths.extend(sorted(candidates))
        if paths:
            break
        # Registry archives are package roots, not shared workspaces; never inspect siblings.
        if (package.get("source") or "").startswith("registry+") or not package.get("source"):
            break
        parent = current.parent
        if parent == current:
            break
        current = parent
    source_root = directory.resolve()
    if (package.get("source") or "").startswith("git+"):
        candidate = directory
        for _ in range(6):
            if (candidate / ".git").exists():
                source_root = candidate.resolve()
                break
            candidate = candidate.parent
    unique = []
    for path in paths:
        if path.is_file() and path not in unique:
            try:
                # A dependency's license_file must not smuggle arbitrary local files
                # into a notice bundle later published with the application.
                path.resolve().relative_to(source_root)
                unique.append(path)
            except ValueError:
                pass
    return unique


def read_license(path):
    if path.stat().st_size > MAX_LICENSE_BYTES:
        raise ValueError("A license text exceeds the inventory size limit.")
    text = path.read_text(encoding="utf-8", errors="strict").replace("\r\n", "\n").strip() + "\n"
    if "\x00" in text:
        raise ValueError("A license resource is not plain text.")
    return text


def upstream_override(package):
    """Resolve audited, checked-in notices when a published crate omits its license files."""
    path = PACKAGING / "license-overrides.json"
    if not path.is_file() or not package.get("source"):
        return None
    overrides = json.loads(path.read_text(encoding="utf-8"))
    if overrides.get("schema_version") != 1:
        raise ValueError("Unsupported license override schema.")
    item = overrides["packages"].get(package["name"] + "@" + package["version"])
    if item is None:
        return None
    if item["declared_license"] != package.get("license") or item["upstream_repository"] != package.get("repository"):
        raise ValueError("An upstream license declaration changed; review the override.")
    directory = pathlib.Path(package["manifest_path"]).parent
    vcs_path = directory / ".cargo_vcs_info.json"
    if vcs_path.is_file():
        vcs = json.loads(vcs_path.read_text(encoding="utf-8"))
        if vcs.get("git", {}).get("sha1") != item["upstream_revision"]:
            raise ValueError("The upstream Cargo revision does not match its license override.")
    for check in item.get("source_checks", []):
        source = directory / check["path"]
        source.resolve().relative_to(directory.resolve())
        if hashlib.sha256(source.read_bytes()).hexdigest() != check["sha256"]:
            raise ValueError("The published source no longer matches its reviewed upstream revision.")
    for notice in item["files"]:
        resource = PACKAGING / notice["path"]
        resource.resolve().relative_to((PACKAGING / "licenses").resolve())
        if resource.stat().st_size > MAX_LICENSE_BYTES or hashlib.sha256(resource.read_bytes()).hexdigest() != notice["sha256"]:
            raise ValueError("A reviewed upstream notice failed its checksum.")
    return item


def collect(metadata, roots, target):
    packages = selected_packages(metadata, roots)
    texts, inventory, gaps, provenance_notes = {}, [], [], []

    def attach(path, label):
        text = read_license(path)
        digest = hashlib.sha256(text.encode()).hexdigest()
        record = texts.setdefault(digest, {"labels": set(), "text": text})
        record["labels"].add(label)
        return {"name": path.name, "sha256": digest}

    for package in packages:
        label = package["name"] + " " + package["version"]
        local = package.get("source") is None
        source_files = license_files(package)
        override = upstream_override(package) if not source_files else None
        if override:
            source_files = [PACKAGING / notice["path"] for notice in override["files"]]
        files = [attach(path, label) for path in source_files]
        item = {"name": package["name"], "version": package["version"], "source": package.get("source") or "Pickle project", "license": package.get("license"), "license_texts": files, "project_code": local}
        if override:
            item["upstream_notice_provenance"] = override
            if override.get("note"):
                provenance_notes.append(label + ": " + override["note"])
        inventory.append(item)
        if not local and (not package.get("license") or not files):
            gaps.append(label)
        if package["name"] == "rune":
            assets = pathlib.Path(package["manifest_path"]).parent / "assets"
            inter = assets / "Inter-LICENSE.txt"
            if inter.is_file():
                attach(inter, "Embedded Inter font family")
            else:
                gaps.append("Embedded Inter font license")
    if "pickle-app" in roots:
        readability = ROOT / "browser-extension" / "Readability-LICENSE.md"
        if readability.is_file():
            attach(readability, "Bundled Mozilla Readability browser extension")
        else:
            gaps.append("Bundled Readability license")
    result = {"schema_version": 1, "target": target, "root_packages": sorted(roots), "scope": "Reachable non-dev Cargo dependencies (normal and build), plus explicitly bundled Inter and Readability assets. Review selected features and non-Cargo components separately.", "packages": inventory, "review_gaps": sorted(set(gaps)), "provenance_notes": sorted(provenance_notes), "license_texts": [{"sha256": digest, "labels": sorted(value["labels"])} for digest, value in sorted(texts.items())]}
    header = ["Pickle third-party notices", "", "Generated from the locked Cargo resolve graph.", "Target: " + target, "", result["scope"], "", "Dependency inventory", "===================="]
    for package in inventory:
        header.append(f"{package['name']} {package['version']}: {package['license'] or 'License not declared'}")
    if result["review_gaps"]:
        header.extend(["", "Manual review required", "======================", *result["review_gaps"]])
    if provenance_notes:
        header.extend(["", "Upstream notice provenance", "==========================", *sorted(provenance_notes)])
    for digest, value in sorted(texts.items()):
        header.extend(["", "=" * 72, ", ".join(sorted(value["labels"])), "SHA-256: " + digest, "", value["text"].rstrip()])
    return result, "\n".join(header) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest-path", type=pathlib.Path, default=ROOT / "rune" / "Cargo.toml")
    parser.add_argument("--package", action="append", dest="packages")
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--strict", action="store_true", help="Return nonzero for missing third-party SPDX/license text.")
    args = parser.parse_args()
    completed = subprocess.run(["cargo", "metadata", "--manifest-path", str(args.manifest_path), "--locked", "--format-version", "1", "--filter-platform", args.target], check=True, stdout=subprocess.PIPE)
    inventory, notices = collect(json.loads(completed.stdout), args.packages or ["pickle-app"], args.target)
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "dependency-inventory.json").write_text(json.dumps(inventory, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (args.output / "THIRD_PARTY_NOTICES.txt").write_text(notices, encoding="utf-8")
    print(f"Inventoried {len(inventory['packages'])} packages and {len(inventory['license_texts'])} distinct notice texts; {len(inventory['review_gaps'])} review gaps.")
    if inventory["review_gaps"]:
        print("Review gaps: " + ", ".join(inventory["review_gaps"]), file=sys.stderr)
        if args.strict:
            return 2
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.CalledProcessError):
        print("Could not generate the dependency notices. Inspect the locked build inputs and license resources.", file=sys.stderr)
        sys.exit(1)
