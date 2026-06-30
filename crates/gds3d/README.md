# GDS3D egui migration spike

This crate is the Rust + egui migration branch for GDS3D. It intentionally
tracks the current PySide application shape instead of replacing it with an
unrelated demo.

Current correspondence:

- `Scene`, `SceneObject`, `GdsLayerObject`, and `BaseplateObject` mirror the
  Python model objects.
- The main window maps to egui top menus, left component tree, central
  viewport, right property panel, and bottom status bar.
- Import/open/export actions are wired at the UI/action layer. GDS parsing,
  true project archive compatibility, and renderer-backed export are explicit
  next migration steps.
- The viewport is currently a lightweight painter-backed 3D projection with
  orbit, pan, zoom, extruded objects, axes, selection highlighting, and depth
  sorting. It is still a migration scaffold before the final `wgpu` renderer.

Run:

```sh
cargo run -p gds3d
```

Recommended next steps:

1. Move GDS inspection/loading into Rust and replace the placeholder import.
2. Replace the painter viewport with an `egui-wgpu` custom renderer.
3. Implement vector SVG/PDF export directly from scene geometry.
4. Add `.gds3d` archive compatibility with the Python archive format.
