# Pickle application icons

`Pickle.svg` is the editable master. Its green seeded pickle and single portal retain the existing application's identity. `Pickle-small.svg` intentionally removes the portal, gradients, and highlight for legibility at 16–32 pixels. Both sources use vector paths only: no fonts, remote resources, or embedded raster images.

Palette:

| Role | sRGB value |
|---|---|
| Forest tile | `#142C20` |
| Deep forest | `#0C1C15` |
| Pickle green | `#B3F540` |
| Light edge | `#D0F77A` |
| Seed marks | `#477427` |
| Quiet portal | `#5CBE9E` |

Rebuild all checked-in output assets from the repository root:

```sh
cargo run --locked --manifest-path rune/tools/icon-builder/Cargo.toml -- rune/packaging/assets
cargo test --locked --manifest-path rune/tools/icon-builder/Cargo.toml
```

The helper uses a pinned, font-free `resvg` renderer, writes PNG frames, and places those frames into deterministic ICO and ICNS containers. Commit its `Cargo.lock` when changing dependencies. No platform-specific graphics tools are required.

Outputs:

- `Pickle.ico`: 16, 20, 24, 32, 40, 48, 64, 128, and 256 pixel images for Windows.
- `Pickle.icns`: standard Mac icon representations from 16 to 1024 pixels, including Retina representations.
- `Pickle-preview.png`: 512-pixel visual review image.
- `Pickle-16.png`: smallest-size visual review image.

Mac packaging should copy `Pickle.icns` into `Contents/Resources` and set `CFBundleIconFile` to `Pickle`. Windows packaging should use `Pickle.ico` for the executable resource, installer/uninstaller icons, shortcuts, and installed-app display icon. Installer branding alone does not set the executable's taskbar or Explorer icon.

These icons are original Pickle project artwork. No third-party icon library or stock artwork was used. The rasterizer is build tooling, and its dependencies should be inventoried separately from the application runtime dependencies.
