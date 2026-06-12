//! Dungeon document: Zelda-scale dungeons authored on a cell grid. Floors
//! hold walkable cells; walls are implied at floor↔empty boundaries; doors
//! sit on edges between two floor cells and generate a wall-with-doorway
//! there. The whole dungeon exports as ONE self-contained asset folder for
//! bestow (shell mesh + scene + templates + sidecars).
//!
//! Pure logic — no GPU, no UI. Mesh generation in [`meshgen`], glb writing
//! in [`glb`], bestow folder export in [`export`].

pub mod export;
pub mod glb;
pub mod meshgen;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Cell {
    #[default]
    Empty,
    Floor,
}

/// One floor (storey) of the dungeon: a `width`×`depth` grid of cells.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Floor {
    pub width: usize,
    pub depth: usize,
    pub cells: Vec<Cell>,
}

impl Floor {
    pub fn new(width: usize, depth: usize) -> Self {
        Self {
            width,
            depth,
            cells: vec![Cell::Empty; width * depth],
        }
    }

    pub fn get(&self, x: i64, z: i64) -> Cell {
        if x < 0 || z < 0 || x as usize >= self.width || z as usize >= self.depth {
            return Cell::Empty;
        }
        self.cells[z as usize * self.width + x as usize]
    }

    pub fn set(&mut self, x: usize, z: usize, cell: Cell) {
        if x < self.width && z < self.depth {
            self.cells[z * self.width + x] = cell;
        }
    }

    pub fn floor_count(&self) -> usize {
        self.cells.iter().filter(|c| **c == Cell::Floor).count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DoorKind {
    #[default]
    Open,
    /// Requires the named key.
    Locked { key: String },
    /// Requires the boss key.
    Boss,
}

impl DoorKind {
    pub fn label(&self) -> &'static str {
        match self {
            DoorKind::Open => "open",
            DoorKind::Locked { .. } => "locked",
            DoorKind::Boss => "boss",
        }
    }
}

/// A door on the edge between two ADJACENT floor cells (same storey). The
/// generator emits a wall with a doorway across that edge; the export
/// emits a door entity so gameplay Lua can open/lock it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Door {
    pub name: String,
    pub floor: usize,
    pub a: (usize, usize),
    pub b: (usize, usize),
    pub kind: DoorKind,
}

/// An entity placed inside the dungeon (template + transform + tags),
/// mirroring wright's placement mode. Positions are dungeon-local meters
/// (same space as the shell mesh).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DungeonEntity {
    pub name: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub position: [f32; 3],
    #[serde(default)]
    pub yaw_deg: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DungeonDoc {
    pub name: String,
    /// Meters per grid cell.
    pub cell_size: f32,
    /// Floor-to-ceiling height of each storey, meters.
    pub wall_height: f32,
    /// Vertical distance between storey floors, meters (>= wall_height).
    pub floor_height: f32,
    /// Generate ceiling quads (almost always true for dungeons).
    pub ceilings: bool,
    /// Doorway opening width / height, meters.
    #[serde(default = "default_door_width")]
    pub door_width: f32,
    #[serde(default = "default_door_height")]
    pub door_height: f32,
    pub floors: Vec<Floor>,
    #[serde(default)]
    pub doors: Vec<Door>,
    #[serde(default)]
    pub entities: Vec<DungeonEntity>,
}

fn default_door_width() -> f32 {
    1.4
}

fn default_door_height() -> f32 {
    2.4
}

impl DungeonDoc {
    pub fn new(name: &str, width: usize, depth: usize) -> Self {
        Self {
            name: name.to_string(),
            cell_size: 2.0,
            wall_height: 4.0,
            floor_height: 5.0,
            ceilings: true,
            door_width: default_door_width(),
            door_height: default_door_height(),
            floors: vec![Floor::new(width, depth)],
            doors: Vec::new(),
            entities: Vec::new(),
        }
    }

    /// World X of the grid's min corner — the dungeon is centred on the
    /// origin in XZ like wright islands (transform places it in a scene).
    pub fn origin_x(&self) -> f32 {
        -(self.floors.first().map_or(0, |f| f.width) as f32) * self.cell_size * 0.5
    }

    pub fn origin_z(&self) -> f32 {
        -(self.floors.first().map_or(0, |f| f.depth) as f32) * self.cell_size * 0.5
    }

    /// World-space centre of a cell's floor.
    pub fn cell_center(&self, floor: usize, x: usize, z: usize) -> [f32; 3] {
        [
            self.origin_x() + (x as f32 + 0.5) * self.cell_size,
            floor as f32 * self.floor_height,
            self.origin_z() + (z as f32 + 0.5) * self.cell_size,
        ]
    }

    /// Grid cell containing a world XZ on the given storey, if in bounds.
    pub fn cell_at(&self, floor: usize, wx: f32, wz: f32) -> Option<(usize, usize)> {
        let f = self.floors.get(floor)?;
        let x = (wx - self.origin_x()) / self.cell_size;
        let z = (wz - self.origin_z()) / self.cell_size;
        if x < 0.0 || z < 0.0 {
            return None;
        }
        let (x, z) = (x as usize, z as usize);
        (x < f.width && z < f.depth).then_some((x, z))
    }

    /// Doorway (width, height) clamped to fit the cell and wall — total
    /// order is enforced so degenerate sizes from hand-edited projects can
    /// never panic `f32::clamp`.
    pub fn door_dims(&self) -> (f32, f32) {
        let max_w = (self.cell_size - 0.2).max(0.4);
        let max_h = (self.wall_height - 0.1).max(1.0);
        (
            self.door_width.clamp(0.4, max_w),
            self.door_height.clamp(1.0, max_h),
        )
    }

    /// Door on the edge between cells `a` and `b`, if any.
    pub fn door_between(
        &self,
        floor: usize,
        a: (usize, usize),
        b: (usize, usize),
    ) -> Option<&Door> {
        self.doors
            .iter()
            .find(|d| d.floor == floor && ((d.a == a && d.b == b) || (d.a == b && d.b == a)))
    }

    /// Problems that would make the exported dungeon broken (errors) or
    /// surprising (warnings). Name checks cover the whole scene namespace:
    /// doors + entities + the reserved `sky`/`shell` entities the export
    /// always emits — bestow's scene load hard-fails on `name_taken`.
    pub fn validate(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        let mut names: HashSet<&str> = HashSet::from(["sky", "shell"]);
        let mut edges = HashSet::new();

        if self.floors.iter().map(Floor::floor_count).sum::<usize>() == 0 {
            issues.push(Issue::error("dungeon has no floor cells"));
        }

        for (what, name) in self
            .doors
            .iter()
            .map(|d| ("door", d.name.as_str()))
            .chain(self.entities.iter().map(|e| ("entity", e.name.as_str())))
        {
            if !name.is_empty() && !names.insert(name) {
                issues.push(Issue::error(format!(
                    "duplicate {what} name `{name}` (names are unique per scene; \
                     `sky` and `shell` are reserved)"
                )));
            }
        }

        for door in &self.doors {
            let key = (door.floor, door.a.min(door.b), door.a.max(door.b));
            if !edges.insert(key) {
                issues.push(Issue::error(format!(
                    "two doors share the edge {:?}–{:?} on storey {}",
                    door.a, door.b, door.floor
                )));
            }
            let Some(floor) = self.floors.get(door.floor) else {
                issues.push(Issue::error(format!(
                    "door `{}` is on storey {} which does not exist",
                    door.name, door.floor
                )));
                continue;
            };
            let (ax, az) = (door.a.0 as i64, door.a.1 as i64);
            let (bx, bz) = (door.b.0 as i64, door.b.1 as i64);
            if (ax - bx).abs() + (az - bz).abs() != 1 {
                issues.push(Issue::error(format!(
                    "door `{}` cells {:?} / {:?} are not adjacent",
                    door.name, door.a, door.b
                )));
            }
            if floor.get(ax, az) != Cell::Floor || floor.get(bx, bz) != Cell::Floor {
                issues.push(Issue::error(format!(
                    "door `{}` must sit between two floor cells",
                    door.name
                )));
            }
            if let DoorKind::Locked { key } = &door.kind {
                let key_placed = self.entities.iter().any(|e| {
                    e.tags.iter().any(|t| t == &format!("key.{key}"))
                        || e.name == *key
                        || e.template == *key
                });
                if !key_placed {
                    issues.push(Issue::warn(format!(
                        "door `{}` needs key `{key}` but no entity provides it \
                         (tag `key.{key}`, or name/template `{key}`)",
                        door.name
                    )));
                }
            }
        }

        if !self
            .entities
            .iter()
            .any(|e| e.tags.iter().any(|t| t == "player_spawn"))
        {
            issues.push(Issue::warn(
                "no entity tagged `player_spawn` — the game won't know where to put the player",
            ));
        }

        for e in &self.entities {
            let storey = (e.position[1] / self.floor_height).floor().max(0.0) as usize;
            if self
                .cell_at(
                    storey.min(self.floors.len().saturating_sub(1)),
                    e.position[0],
                    e.position[2],
                )
                .is_none()
            {
                issues.push(Issue::warn(format!(
                    "entity `{}` sits outside the grid",
                    e.name
                )));
            }
        }

        issues
    }

    // ── project on disk ──────────────────────────────────────────────────

    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let s = toml::to_string_pretty(self)?;
        std::fs::write(path, s)?;
        Ok(())
    }

    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let doc: Self = toml::from_str(&std::fs::read_to_string(path)?)?;
        // Structural checks: a hand-edited project must not panic the
        // editor (indexing floors[0], cells[z*w+x], dividing by sizes).
        anyhow::ensure!(!doc.floors.is_empty(), "project has no storeys");
        for (i, f) in doc.floors.iter().enumerate() {
            anyhow::ensure!(
                f.width >= 1 && f.depth >= 1 && f.cells.len() == f.width * f.depth,
                "storey {i}: cells length {} does not match {}x{}",
                f.cells.len(),
                f.width,
                f.depth
            );
        }
        anyhow::ensure!(
            doc.cell_size.is_finite() && doc.cell_size >= 0.5,
            "cell_size must be at least 0.5 m"
        );
        anyhow::ensure!(
            doc.wall_height.is_finite() && doc.wall_height >= 1.0,
            "wall_height must be at least 1 m"
        );
        anyhow::ensure!(
            doc.floor_height.is_finite() && doc.floor_height >= doc.wall_height,
            "floor_height must be >= wall_height"
        );
        Ok(doc)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub error: bool,
    pub message: String,
}

impl Issue {
    fn error(m: impl Into<String>) -> Self {
        Self {
            error: true,
            message: m.into(),
        }
    }

    fn warn(m: impl Into<String>) -> Self {
        Self {
            error: false,
            message: m.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_room_dungeon() -> DungeonDoc {
        // two 3x3 rooms joined by a 1-wide corridor with a door on it
        let mut doc = DungeonDoc::new("test", 9, 3);
        let f = &mut doc.floors[0];
        for z in 0..3 {
            for x in 0..3 {
                f.set(x, z, Cell::Floor); // west room
                f.set(x + 6, z, Cell::Floor); // east room
            }
        }
        f.set(3, 1, Cell::Floor); // corridor
        f.set(4, 1, Cell::Floor);
        f.set(5, 1, Cell::Floor);
        doc.doors.push(Door {
            name: "east_door".into(),
            floor: 0,
            a: (4, 1),
            b: (5, 1),
            kind: DoorKind::Locked {
                key: "small_key".into(),
            },
        });
        doc
    }

    #[test]
    fn validation_catches_problems() {
        let mut doc = two_room_dungeon();
        // missing key + missing player_spawn -> two warnings, no errors
        let issues = doc.validate();
        assert_eq!(issues.iter().filter(|i| i.error).count(), 0);
        assert_eq!(issues.iter().filter(|i| !i.error).count(), 2);

        // fix both
        doc.entities.push(DungeonEntity {
            name: "spawn".into(),
            template: String::new(),
            tags: vec!["player_spawn".into()],
            position: doc.cell_center(0, 1, 1),
            yaw_deg: 0.0,
        });
        doc.entities.push(DungeonEntity {
            name: "key1".into(),
            template: "chest".into(),
            tags: vec!["key.small_key".into()],
            position: doc.cell_center(0, 7, 1),
            yaw_deg: 0.0,
        });
        assert!(doc.validate().is_empty());

        // non-adjacent door cells -> error
        doc.doors.push(Door {
            name: "broken".into(),
            floor: 0,
            a: (0, 0),
            b: (2, 0),
            kind: DoorKind::Open,
        });
        assert!(doc.validate().iter().any(|i| i.error));
    }

    #[test]
    fn grid_world_mapping_roundtrips() {
        let doc = two_room_dungeon(); // 9x3 cells of 2m: x in [-9, 9], z in [-3, 3]
        assert_eq!(doc.origin_x(), -9.0);
        let c = doc.cell_center(0, 0, 0);
        assert_eq!(c, [-8.0, 0.0, -2.0]);
        assert_eq!(doc.cell_at(0, -8.0, -2.0), Some((0, 0)));
        assert_eq!(doc.cell_at(0, 8.9, 2.9), Some((8, 2)));
        assert_eq!(doc.cell_at(0, -9.5, 0.0), None);
    }

    #[test]
    fn project_roundtrip() {
        let doc = two_room_dungeon();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("test.wrightdungeon");
        doc.save(&p).unwrap();
        let back = DungeonDoc::load(&p).unwrap();
        assert_eq!(back.floors[0].floor_count(), doc.floors[0].floor_count());
        assert_eq!(
            back.doors[0].kind,
            DoorKind::Locked {
                key: "small_key".into()
            }
        );
    }
}
