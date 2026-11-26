# Portal Lab

A Vite and React workbench scene drawn with inline SVG. From this directory, use Node 22.12 or later and run:

```sh
npm ci
npm run dev
```

Open the local URL printed by Vite. Click the green CRT screen, or focus it and press Enter, to zoom toward the spinning portal. The observatory opens inside the glass and lays out its page at the enlarged screen size. Its return link or browser Back zooms out to the original lab view. Reduced-motion mode switches views immediately. The small power light beneath the CRT toggles the ambient animation independently.

Run `npm run build` to create the static site in `dist/` and `npm run preview` to serve that build locally.

## Edit the scene

`src/PortalLab.jsx` composes the objects and controls portal motion. Each of the 27 objects has its own SVG file in `src/artwork/`. Their order in `layers.json` runs from background to foreground. Vite embeds the path definitions in the page, and React positions them in the 2048 × 1152 scene.

The original PNG is a tracing reference. To regenerate the vector layers after changing the silhouettes, install Pillow and vtracer in a Python environment and run `python build_scene.py workbench.png src/artwork`. Regenerating replaces edits made directly to the SVG files.

The vertical extensions are separate SVG drawings in `src/artwork/rafters.svg` and `src/artwork/bench-cabinet.svg`. Two built-in ImageGen painting passes supplied their reference art: first extend the original with attic rafters, porcelain wire mounts, a chipped cabinet and electrical salvage; then extend the distant roof and floor while preserving the first painting's perspective. The browser loads these vectors when there is space above and below the workbench.

To rebuild the extensions, install NumPy alongside Pillow and vtracer and run:

```sh
python build_painted_surround.py reference/workshop-portrait.png reference/workshop-tall.png
```
