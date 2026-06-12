# wright

**The standalone editor that crafts visual resources for [bestow](../bestow).**

A *wright* is a maker — shipwright, playwright. Bestow's design forbids it
from ever growing an editor (its D-005: *the game is the visualizer, never
the editor*; data flows disk → engine). wright is the other half of that
bargain: a dedicated desktop editor that authors game resources and exports
them in exactly the formats bestow consumes, so the engine never needs
editing UI of its own.

## What it does today

**Island mode** — sculpt heightfield islands and export them ready-to-load:

- Brushes: raise, lower, flatten, smooth, noise — with radius / strength /
  falloff, smoothstep falloff like argh's Terrain Sculpt
- Material painting: rock/grass with autoshader override (slope-driven
  blending where unpainted, matching `island_baked.slang`), plus RGB tint
- Full undo/redo (region-patch history, 256 strokes)
- Live 3D viewport: orbit/pan/zoom camera, brush ring decal, water plane,
  chunked remeshing so only the chunks you touch rebuild
- Lossless `.wright` project format (raw f32 heights + full-depth masks)
- **Export to bestow**: `<name>.hgt.png` (16-bit grayscale), `<name>.hgt.toml`
  (placement metadata), `<name>.ctl.png` (R=rockness, G=autoshader),
  `<name>.color.png` (tint), UUIDv7 `.import.toml` sidecars (stable across
  re-export), and a ready-to-paste `[[entities]]` scene snippet

Sculpt with a running bestow pointed at the same assets dir and hot reload
shows your island in-game seconds after each export. Existing bestow
islands (including ones originally dumped from argh) re-import for editing
via their `.hgt.toml`.

**Animation mode** — load a glTF rig and author the metadata bestow needs:

- Preview playback on a bone-line skeleton: scrub, play/pause (space),
  loop, speed; click a joint (or the tree) to select a bone
- Sockets: named attachment points on bones with offset editing, drawn as
  gizmo tripods (`attach_socket` targets for bestow templates)
- Event tags: named events with payloads at exact clip times
  (`[[animation.clips.events]]`) — gold markers on the timeline
- Clip splitting: mark in/out to cut sections (`[[animation.clips.sections]]`)
  with per-section `can_end` for player-cancellable combos — teal spans on
  the timeline
- Metadata saves as `.wrightanim` (TOML) and exports as bestow
  `<model>.anim.toml`

**Dungeon mode** — Zelda-scale dungeons as single drop-in asset folders:

- Paint walkable cells on a grid (multi-storey), erase, place doors on
  edges between floor cells — open / locked-with-key / boss — and place
  entities (templates or tag-only markers like `player_spawn`)
- Live 3D shell preview: walls auto-generated at floor↔empty boundaries
  with real doorway openings; backface culling gives a dollhouse view from
  above; live validation (door adjacency, missing keys, missing spawn)
- Exports ONE self-contained folder under `assets/dungeons/<name>/`:
  scene TOML + shell `.glb` (visuals AND trimesh collision) + a
  path-qualified door template + UUIDv7 sidecars. Play it with
  `scene.load("assets/dungeons/<name>/<name>.scene.toml")` — enter, exit,
  re-enter, and same-frame resets all verified against a live bestow

**Placement mode** — arrange entities on exported islands:

- Load any exported island as the ground, click terrain to place entity
  templates (snapped to the surface), with a recent-template palette
- Per-entity name / template / tags / yaw editing; flag-pole markers with
  yaw direction ticks; click markers to select
- Exports bestow `[[entities]]` scene TOML blocks; projects save as
  `.wrightscene`

## Controls

| Input | Action |
|---|---|
| LMB drag | apply brush |
| RMB drag | orbit camera |
| MMB / Shift+RMB drag | pan |
| scroll | zoom |
| `F` | frame island |
| `1`–`9` | select brush |
| `[` `]` | brush radius |
| ⌘Z / ⇧⌘Z | undo / redo |
| ⌘S | save project |

## Build & run

```sh
cargo run --bin wright
```

Workspace layout:

- `crates/wright-field` — heightfield document model: brushes, masks,
  chunked meshing, ray picking. Pure logic, fully unit-tested.
- `crates/wright-bestow` — export pipeline into bestow's on-disk formats.
- `crates/wright-app` — the eframe/egui + wgpu editor application.

Editor state (recent project, export dir) lives at
`~/.local/share/wright/state.toml`.

## What's next

See [ROADMAP.md](ROADMAP.md): animation tooling (socket placement, event
tags, clip splitting for combos, previews), dungeon crafter, entity
placement — every mode exporting straight into bestow formats.

## License

MIT OR Apache-2.0, like bestow. All dependencies permissively licensed.
