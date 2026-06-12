# Open questions — dungeon mode

Decisions I made to keep moving (the dungeon slice is shipped and
verified end-to-end against bestow); circle back whenever. Each lists
what I chose and what changing it would take.

## Resolved by implementation — confirm the defaults suit you

1. **Cell size / wall height.** Defaults 2 m cells, 4 m walls, 1.4×2.4 m
   doorways — all per-dungeon settings in the New dialog and project file.

2. **Doors.** Doors sit on an edge between two floor cells and generate a
   wall-with-doorway there. Locked/boss doors export as *blocking slab
   entities* (cube visual + static box collider, tagged `door.locked` /
   `door.boss` + `key.<id>`); **open doorways export as marker entities
   only** so nothing blocks them when no game logic runs. Gameplay (the
   actual opening/unlocking) stays in your game's Lua — wright exports
   data, never behavior. The door template ships inside the dungeon
   folder and is referenced by full path (I added path-qualified template
   refs to bestow so two dungeons can both ship a `door` template).

3. **Enter/exit contract.** The outer game calls
   `scene.load("assets/dungeons/<name>/<name>.scene.toml")`, teleports the
   player to the entity tagged `player_spawn`, and reverses on exit.
   Verified working in bestow including same-frame unload→reload (dungeon
   reset) — that needed a small engine fix (names now free at despawn
   time, not at the deferred sync point).

4. **Collision.** I added `shape = "mesh"` trimesh colliders to bestow
   (the documented-but-unimplemented descriptor in physics.md): the
   runtime cooks a fixed trimesh from the shell glb, same lifecycle as
   terrain heightfields (re-anchor on move, re-cook on hot reload). The
   dungeon shell is ONE entity with visuals and collision from the same
   glb. Note: bestow is not a git repo, so these engine changes are
   sitting uncommitted in its working tree.

## Genuinely open

5. **Multi-storey connections.** The model and editor support multiple
   storeys, but there's no stair *geometry* yet — the Zelda-idiomatic
   pattern today is stair/ladder marker entities + a game-side teleport.
   Want real ramp/stair geometry cut between storeys, or is the
   marker+teleport pattern actually what you'd use anyway?

6. **Dungeon lighting.** Exported scenes include a dim "cavern" sky
   entity so dungeons don't look sunlit. Torch placement is deferred
   because bestow caps 8 active point lights — if torch-lined halls
   matter, the engine wants nearest-N light culling (Rust-side, small)
   before wright grows a torch tool. Worth doing?

7. **Shell materials.** The shell glb carries flat per-surface colors
   (floor/wall/ceiling). Texture-mapped dungeon materials (and a paint
   tool like the island rockness brush) are the natural next slice —
   which matters more to you: dungeon wall texturing, or stairs (Q5)?

8. **Relocatability.** A dungeon folder is drop-in but not movable after
   export (asset paths are game-root-relative, per bestow's own
   convention). Renaming/moving means re-exporting under the new name.
   Fine, or do you want scene-relative path resolution in bestow?
