# Release dependency and asset notices

This file describes the release inventory process. The generated inventory and complete notice bundle must accompany packaged releases; this checklist is not a substitute for their contents.

## Generate the release inventory

From the repository root, after resolving and committing the application lockfile:

```sh
python3 rune/packaging/generate-notices.py \
  --target aarch64-apple-darwin \
  --output rune/artifacts/notices-macos \
  --strict

python3 rune/packaging/generate-notices.py \
  --target x86_64-pc-windows-msvc \
  --output rune/artifacts/notices-windows \
  --strict
```

Run this with the same toolchain, locked dependencies, target, and features as the release build. The helper traverses reachable normal/build Cargo dependencies from `pickle-app`, excludes dev-only edges, collects full license files, and writes:

- `dependency-inventory.json`: package names, versions, source URLs, declared SPDX expressions, notice-file hashes, and unresolved review gaps.
- `THIRD_PARTY_NOTICES.txt`: the inventory plus deduplicated full license/copyright texts.

Cargo metadata inventories source dependencies; it does not prove which code survives linking. Build dependencies are included conservatively. Feature-specific and non-Cargo bundled assets need separate review. Missing third-party declarations or license text cause `--strict` to fail instead of silently producing a complete-looking bundle.

Some published crates omit repository-level license files. `license-overrides.json` supplies audited notices copied from exact upstream Cargo VCS revisions into `licenses/`, with original source URLs and SHA-256 checksums. The generator verifies those checksums and matches the package's declared license, repository, and VCS revision before using an override. For older crates without Cargo VCS metadata, the override records a version tag or a published-source checksum matched to a specific upstream commit.

Some upstream repositories also omit full license bodies, or provide declarations linking to standard licenses. Those overrides preserve the actual upstream declarations and available copyright headers, and separately include verbatim canonical SPDX license texts pinned to the recorded SPDX commit. Template placeholders in canonical texts are left unchanged; no copyright year or holder is invented. Generated `provenance_notes` identify these cases. This is a record of upstream declarations and license references, not a new license grant or a substitute for the release owner's distribution review. The complete upstream objc2 licensing caveat is retained.

The original Pickle project's own code has no declared project-wide license in the inspected repository. The release owner must decide its distribution terms; do not relabel it as MIT merely because Rune is MIT licensed.

## Components requiring explicit asset review

| Component | Inventory treatment |
|---|---|
| Rune and its companion crates | Pinned Git revision; declared license plus the repository's full license text |
| Rune's patched/vendored dependencies | Individually inventoried from Cargo's resolve graph; retain their upstream notices |
| Embedded Inter fonts | The helper adds Rune's `Inter-LICENSE.txt`; retain the complete SIL OFL notice with the native release |
| IBM Plex Mono | Rune's inspected native default embeds Inter; if Plex is added or a web build is shipped, add its complete font notice |
| Readability | The helper includes the bundled browser extension's `Readability-LICENSE.md` |
| Rune math/typesetting assets | Not enabled by the current `markdown` feature alone; if enabled, include Rune's complete math/font notices and separately embedded WASM dependencies |
| Original Pickle icon assets | Original vector artwork; no stock icons, external images, or font dependency |
| Icon renderer | Build-only standalone Rust tool; inventory separately if its executable/source distribution is included |
| Rust runtime/system components | Review the selected Rust toolchain and platform redistribution requirements separately from the Cargo package graph |
| Installer/updater components | Review the specific NSIS/MSIX/updater binaries and plugins used by release packaging |
| Local inference/transcription models | The app must not implicitly redistribute weights; separately review any model/executable later bundled with a release |

## Release checklist

- [ ] Generate the inventory from the actual release lockfile for each target.
- [ ] Resolve every `review_gaps` item and record any non-Cargo additions.
- [ ] Confirm optional features match the inventory assumptions.
- [ ] Include the full notices and inventory in the Mac resources and Windows install directory.
- [ ] Preserve the browser extension's existing Readability notice.
- [ ] Confirm installer/uninstaller cleanup includes only these application-owned generated notice files.
- [ ] Archive the inventory, artifact hashes, Rune revision, and source commit together.
- [ ] Review licensing/distribution terms with the release owner before publishing.
