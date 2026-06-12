# CLAUDE.md — wright

Guidance for Claude Code when working in the wright repository.

## What wright is

The standalone editor for **bestow** resources. Bestow never gets an editor
(its D-005); wright authors visual game data — islands, animation metadata,
dungeons, entity placement — and **exports files in exactly the formats
bestow consumes**. The export contract is the product; the UI exists to
produce those files pleasantly.

## Non-negotiables

1. **Export formats are bestow's, verbatim.** Before changing an exporter,
   check what bestow actually parses (`bestow-runtime/src/terrain.rs`,
   `games/isles/assets/islands/`, docs/systems/*.md in the bestow repo).
   If bestow and its docs disagree, the code wins.
2. **UUIDv7 sidecars never churn.** An existing `.import.toml` id is
   preserved on re-export — bestow tracks assets by that id (D-011).
3. **Projects are lossless.** `.wright` project files keep full-precision
   data (f32 heights, full masks). Quantization happens only at export.
4. **Pure logic stays out of the app crate.** Brushes, meshing, picking,
   exporters live in `wright-field` / `wright-bestow` with unit tests; the
   app crate is UI + GPU plumbing only.
5. **NO dates, NO time-based metrics** — same rule as bestow. Sequencing
   only ("A before B"), in ROADMAP.md, commits, and conversation.
6. **Permissive licenses only** (MIT/Apache-2.0/BSD/zlib/ISC). wright is
   dual-licensed MIT/Apache-2.0.

## Build / test

- `cargo build` / `cargo test --workspace` / `cargo clippy --workspace` /
  `cargo fmt --all`
- Run the editor: `cargo run --bin wright`
- Stack: eframe/egui 0.34 + wgpu 29 (via `eframe::egui_wgpu::wgpu` re-export
  — never add a direct wgpu dep that could drift from egui's).
  eframe 0.34 uses `App::ui(&mut self, ui, frame)` + `Panel::show_inside`.

## Architecture

- `crates/wright-field` — `Heightfield` (f32 grid, world meters, centred on
  origin), `Masks` (rockness/autoshader/tint), `Brush`/`Stroke` (smoothstep
  falloff, dirty `Region` tracking), chunked `Mesher` (64-quad chunks,
  central-difference normals), CPU `raycast` (also hits the y=0 sea plane so
  land can be pulled from the water).
- `crates/wright-bestow` — `export_island` writes `.hgt.png` (16-bit gray),
  `.hgt.toml`, `.ctl.png` (R=rockness, G=autoshader), `.color.png`,
  `.import.toml` sidecars, `.entity.toml` snippet. Atomic writes (tmp +
  rename) so a watching bestow never half-reads. Also `SceneDoc` →
  `[[entities]]` scene TOML for placement mode.
- `crates/wright-anim` — glTF rig loading (skin joints parent-first,
  per-property channels merged onto unified key timelines), sampling that
  mirrors bestow-anim, `AnimMeta` (sockets/events/sections) → bestow TOML.
- `crates/wright-dungeon` — cell-grid dungeon doc (multi-storey, doors on
  edges, validation), shell meshgen (auto walls, doorway jambs/lintel/
  reveals), hand-rolled glb writer (tested by gltf-crate readback),
  `export_dungeon` → one self-contained `assets/dungeons/<name>/` folder.
  The shell glb doubles as bestow's `shape = "mesh"` trimesh collider.
- `crates/wright-app` — eframe app. `modes.rs` is the mode switcher (Island
  live; Animation/Dungeon/Placement are stubs awaiting their slice).
  `island/` owns the document + undo (full-field snapshot at stroke start,
  region patch at commit). `render/` is the offscreen wgpu viewport
  (render-to-texture because egui's pass has no depth buffer; terrain +
  water WGSL in `scene.wgsl` deliberately mirrors `island_baked.slang`
  semantics so the preview matches the in-game look).

## Conventions

- Units: meters, radians; right-handed, Y-up, −Z forward (bestow's).
- Heightfields are square, centred on the world origin; `resolution` is
  samples per side; cell pitch = `world_size / (resolution - 1)`.
- Editor state goes to `~/.local/share/wright/` (XDG, hard-coded).
- Keep brush/mesh logic deterministic (hash-based noise, no RNG state) —
  reproducibility is a feature.
