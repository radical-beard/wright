# wright roadmap

Sequencing only — never durations or dates. Items are ordered within each
mode; modes can interleave. This list grows as new resource types come up;
it is explicitly not exhaustive.

## Island mode (shipped, evolving)

- [x] Heightfield sculpting: raise / lower / flatten / smooth / noise
- [x] Material paint: rockness, autoshader mask, RGB tint
- [x] Undo/redo, lossless project save/load
- [x] Bestow export (hgt/ctl/color PNGs + TOML + UUIDv7 sidecars + entity snippet)
- [ ] Import existing bestow islands (`.hgt.png` + `.hgt.toml`) for re-editing
- [ ] Stamp brushes (mountain/crater/ridge stamps from height stamps)
- [ ] Hole masks (`hole_mask` PNG) for caves and arch cutouts
- [ ] SDF sculpt volumes (argh's cave/arch system) exported as glTF meshes
- [ ] Erosion simulation pass (hydraulic + thermal)
- [ ] N-layer splat painting targeting bestow's full 16-layer terrain schema
- [ ] Live re-export on stroke commit ("link mode": bestow hot-reloads as you sculpt)

## Animation mode

- [ ] Load skeletal models (glTF first; FBX via ufbx parity later)
- [ ] Preview playback: scrub, loop, speed, bone overlay
- [ ] Socket placement: select bone, place named socket with offset gizmo,
      export attachment metadata (`attach_socket` ready)
- [ ] Event tagging: markers on the clip timeline → bestow
      `[[animation.clips.events]]` (name, time, payload)
- [ ] Clip splitting: cut source clips into named segments
      (`[[animation.clips.sections]]`) for combos with early-out points
- [ ] Blend-tree / animgraph authoring → `.animgraph.toml`

## Dungeon mode

- [ ] Grid room/corridor layout with prefab piece palette
- [ ] Door/connection graph with lock-and-key annotations
- [ ] Per-room entity spawn sets
- [ ] Export: scene TOML (`[[entities]]` + `[[includes]]`) bestow loads directly

## Placement mode

- [ ] Load an exported island/dungeon as the ground
- [ ] Place entity templates with snap-to-terrain + rotation/scale gizmos
- [ ] Spawner volumes: tags, counts, radii
- [ ] Scatter brush (foliage/props density painting)
- [ ] Export: `[[entities]]` scene TOML blocks / full `.scene.toml`

## Cross-cutting

- [ ] Multi-document tabs per mode
- [ ] Autosave + crash recovery
- [ ] Bestow game "link": pick a game dir once, all exports land in the
      right asset roots with correct prefixes
- [ ] Screenshot/turntable capture for sharing
