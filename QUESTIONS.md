# Open questions — dungeon mode

Decisions I made to keep moving; circle back whenever. Each lists the
default I chose and why.

## Authoring model

1. **Cell size / wall height.** I defaulted to 2 m cells and 4 m walls
   (classic action-adventure proportions; both are per-dungeon settings in
   the editor, so this is just the default). Good?

2. **Multi-floor.** The document model supports multiple floors. V1
   geometry generates each floor as its own shell with stair cells cutting
   openings between floors. If you mostly want single-floor dungeons with
   separate scenes per floor (Zelda often does this per-wing), say so and
   I'll simplify.

3. **Doors.** Doors live on cell edges between two floor cells and come in
   kinds: `open` (doorway), `locked` (needs a key id), `boss` (boss key).
   The exported scene contains a door entity per door so gameplay Lua can
   open/lock them; the shell mesh leaves a doorway gap. Is key/lock logic
   something wright should *author* only (ids + placement), with the
   gameplay behavior living in your game's Lua? That's my assumption —
   wright exports data, never behavior.

## Export / bestow integration

4. **One folder per dungeon.** Export target is
   `assets/dungeons/<name>/` containing the scene TOML, the shell `.glb`,
   templates for door/chest/spawn markers, and UUIDv7 sidecars. The scene
   references everything by game-root-relative paths, so the folder is
   drop-in but not relocatable after export (bestow asset paths are
   root-relative). If you want relocatable folders (move/rename after
   export without re-export), bestow would need scene-relative path
   resolution — flagging rather than building it speculatively.

5. **Scene switching.** Playing a dungeon "for a section of the game"
   needs runtime scene switching in bestow. If the engine lacks it, I'll
   add a minimal `scene.load(path)` Lua API (Rust does the swap, Lua
   directs it) — replacing the current scene's disk-defined entities,
   same reconciliation path as hot reload. Confirm that's the semantics
   you want (vs. additive loading of a dungeon INTO the current world).

6. **Dungeon lighting.** Dungeons are indoors; the isles/demo look is
   sun + sky. I'll expose whatever per-scene light controls bestow has
   today and otherwise leave lighting to the game. Torch/point-light
   placement as dungeon entities is on the roadmap, not v1, unless you
   want it sooner.

## Gameplay contract

7. **Entering/exiting.** My assumption: the OUTER game decides when to
   enter (e.g. player touches a dungeon entrance), calls the
   scene-switch API with the dungeon's scene path, and the dungeon scene
   contains a `player_spawn`-tagged marker entity where the game places
   the player. Exiting is the same in reverse (an `exit`-tagged marker +
   the game switching back). wright just guarantees those markers exist.
   Match your intent?
