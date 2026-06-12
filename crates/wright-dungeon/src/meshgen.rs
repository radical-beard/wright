//! Dungeon shell mesh from the cell grid. Walls appear at floor↔empty
//! boundaries facing into the room; a door edge between two floor cells
//! produces a thin wall on each side with a doorway opening (jambs +
//! lintel) and reveal quads bridging the two faces, so doorways read as
//! real openings instead of paper-thin slits.
//!
//! Conventions: meters, Y-up, right-handed; glTF CCW front faces; UVs are
//! world-space meters (u along the surface, v = height for walls) so
//! tiling textures land at real scale.

use crate::{Cell, DungeonDoc};
use glam::Vec3;

/// Which material slot a primitive belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Floor,
    Wall,
    Ceiling,
}

#[derive(Debug, Default)]
pub struct Primitive {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl Primitive {
    /// One quad: corners CCW around `normal`, uvs matching corners.
    fn quad(&mut self, corners: [Vec3; 4], normal: Vec3, uvs: [[f32; 2]; 4]) {
        let base = self.positions.len() as u32;
        for (c, uv) in corners.iter().zip(uvs) {
            self.positions.push(c.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

pub struct ShellMesh {
    pub floor: Primitive,
    pub wall: Primitive,
    pub ceiling: Primitive,
}

impl ShellMesh {
    pub fn triangle_count(&self) -> usize {
        self.floor.triangle_count() + self.wall.triangle_count() + self.ceiling.triangle_count()
    }
}

/// Half the gap between the two faces of a door wall (total thickness 0.2m).
const DOOR_WALL_HALF: f32 = 0.1;

pub fn generate(doc: &DungeonDoc) -> ShellMesh {
    let mut mesh = ShellMesh {
        floor: Primitive::default(),
        wall: Primitive::default(),
        ceiling: Primitive::default(),
    };
    let cs = doc.cell_size;
    let (ox, oz) = (doc.origin_x(), doc.origin_z());

    for (fi, floor) in doc.floors.iter().enumerate() {
        let y0 = fi as f32 * doc.floor_height;
        let y1 = y0 + doc.wall_height;

        for z in 0..floor.depth {
            for x in 0..floor.width {
                if floor.get(x as i64, z as i64) != Cell::Floor {
                    continue;
                }
                let (x0, z0) = (ox + x as f32 * cs, oz + z as f32 * cs);
                let (x1, z1) = (x0 + cs, z0 + cs);

                // floor (+Y) and ceiling (−Y, faces down into the room)
                mesh.floor.quad(
                    [
                        Vec3::new(x0, y0, z0),
                        Vec3::new(x0, y0, z1),
                        Vec3::new(x1, y0, z1),
                        Vec3::new(x1, y0, z0),
                    ],
                    Vec3::Y,
                    [[x0, z0], [x0, z1], [x1, z1], [x1, z0]],
                );
                if doc.ceilings {
                    mesh.ceiling.quad(
                        [
                            Vec3::new(x0, y1, z0),
                            Vec3::new(x1, y1, z0),
                            Vec3::new(x1, y1, z1),
                            Vec3::new(x0, y1, z1),
                        ],
                        Vec3::NEG_Y,
                        [[x0, z0], [x1, z0], [x1, z1], [x0, z1]],
                    );
                }

                // walls: per edge — (neighbor dx,dz, bottom-edge start, direction)
                // chosen so the rule normal = (-dz, 0, dx) faces INTO the cell
                let edges = [
                    (
                        (1i64, 0i64),
                        Vec3::new(x1, y0, z0),
                        Vec3::new(0.0, 0.0, 1.0),
                    ),
                    ((-1, 0), Vec3::new(x0, y0, z1), Vec3::new(0.0, 0.0, -1.0)),
                    ((0, 1), Vec3::new(x1, y0, z1), Vec3::new(-1.0, 0.0, 0.0)),
                    ((0, -1), Vec3::new(x0, y0, z0), Vec3::new(1.0, 0.0, 0.0)),
                ];
                for ((dx, dz), start, dir) in edges {
                    let neighbor = floor.get(x as i64 + dx, z as i64 + dz);
                    let door = doc.door_between(
                        fi,
                        (x, z),
                        ((x as i64 + dx) as usize, (z as i64 + dz) as usize),
                    );
                    match (neighbor, door) {
                        (Cell::Floor, None) => {} // open space, no wall
                        (Cell::Floor, Some(_)) => {
                            // door wall: this cell's face, pulled inward
                            let inward = Vec3::new(-dir.z, 0.0, dir.x);
                            let face_start = start + inward * DOOR_WALL_HALF;
                            doorway_wall(doc, &mut mesh.wall, face_start, dir, y0, y1);
                            doorway_reveals(doc, &mut mesh.wall, start, dir, y0);
                        }
                        (Cell::Empty, _) => {
                            wall_quad(&mut mesh.wall, start, dir, cs, y0, y1);
                        }
                    }
                }
            }
        }
    }
    mesh
}

/// Solid wall: bottom edge from `start` along `dir` for `len` meters,
/// spanning y0..y1. Front face is on the side the rule normal points to.
fn wall_quad(prim: &mut Primitive, start: Vec3, dir: Vec3, len: f32, y0: f32, y1: f32) {
    let end = start + dir * len;
    let normal = Vec3::new(-dir.z, 0.0, dir.x);
    let u = |p: Vec3| p.x * dir.x.abs() + p.z * dir.z.abs(); // world coord along the wall axis
    prim.quad(
        [
            start,
            end,
            Vec3::new(end.x, y1, end.z),
            Vec3::new(start.x, y1, start.z),
        ],
        normal,
        [[u(start), y0], [u(end), y0], [u(end), y1], [u(start), y1]],
    );
}

/// Wall with a centred doorway opening: left jamb, right jamb, lintel.
fn doorway_wall(doc: &DungeonDoc, prim: &mut Primitive, start: Vec3, dir: Vec3, y0: f32, y1: f32) {
    let cs = doc.cell_size;
    let (dw, dh) = doc.door_dims();
    let jamb = (cs - dw) * 0.5;

    wall_quad(prim, start, dir, jamb, y0, y1); // left jamb
    wall_quad(prim, start + dir * (jamb + dw), dir, jamb, y0, y1); // right jamb
    // lintel above the opening
    let lintel_start = start + dir * jamb;
    wall_quad(
        prim,
        Vec3::new(lintel_start.x, y0 + dh, lintel_start.z),
        dir,
        dw,
        y0 + dh,
        y1,
    );
}

/// The inner faces of a doorway: two side reveals + the lintel underside,
/// bridging the two offset wall faces. Emitted once per cell side; each
/// side emits the reveal half facing it, so both cells together close the
/// opening. To keep it simple each side emits full-depth reveals facing
/// its own room — the doubled coplanar quads share winding direction per
/// viewer side, so nothing fights.
fn doorway_reveals(doc: &DungeonDoc, prim: &mut Primitive, edge_start: Vec3, dir: Vec3, y0: f32) {
    let cs = doc.cell_size;
    let (dw, dh) = doc.door_dims();
    let jamb = (cs - dw) * 0.5;
    let inward = Vec3::new(-dir.z, 0.0, dir.x);

    let left = edge_start + dir * jamb;
    let right = edge_start + dir * (jamb + dw);
    let (a, b) = (
        left + inward * DOOR_WALL_HALF,
        left - inward * DOOR_WALL_HALF,
    );
    // left reveal: faces the opening (+dir side)
    prim.quad(
        [
            a,
            b,
            Vec3::new(b.x, y0 + dh, b.z),
            Vec3::new(a.x, y0 + dh, a.z),
        ],
        dir,
        [
            [0.0, y0],
            [2.0 * DOOR_WALL_HALF, y0],
            [2.0 * DOOR_WALL_HALF, y0 + dh],
            [0.0, y0 + dh],
        ],
    );
    // right reveal: faces the opening (−dir side)
    let (c, d) = (
        right - inward * DOOR_WALL_HALF,
        right + inward * DOOR_WALL_HALF,
    );
    prim.quad(
        [
            c,
            d,
            Vec3::new(d.x, y0 + dh, d.z),
            Vec3::new(c.x, y0 + dh, c.z),
        ],
        -dir,
        [
            [0.0, y0],
            [2.0 * DOOR_WALL_HALF, y0],
            [2.0 * DOOR_WALL_HALF, y0 + dh],
            [0.0, y0 + dh],
        ],
    );
    // lintel underside (faces down)
    let la = Vec3::new(left.x, y0 + dh, left.z) + inward * DOOR_WALL_HALF;
    let lb = Vec3::new(right.x, y0 + dh, right.z) + inward * DOOR_WALL_HALF;
    let lc = lb - inward * 2.0 * DOOR_WALL_HALF;
    let ld = la - inward * 2.0 * DOOR_WALL_HALF;
    prim.quad(
        [la, ld, lc, lb],
        Vec3::NEG_Y,
        [
            [0.0, 0.0],
            [0.0, 2.0 * DOOR_WALL_HALF],
            [dw, 2.0 * DOOR_WALL_HALF],
            [dw, 0.0],
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Door, DoorKind};

    fn doc_with(corridor_door: bool) -> DungeonDoc {
        let mut doc = DungeonDoc::new("m", 5, 3);
        let f = &mut doc.floors[0];
        for z in 0..3 {
            for x in 0..5 {
                f.set(x, z, Cell::Floor);
            }
        }
        if corridor_door {
            doc.doors.push(Door {
                name: "d".into(),
                floor: 0,
                a: (2, 1),
                b: (3, 1),
                kind: DoorKind::Open,
            });
        }
        doc
    }

    fn check_winding(prim: &Primitive) {
        for tri in prim.indices.chunks_exact(3) {
            let p = |i: u32| Vec3::from_array(prim.positions[i as usize]);
            let n = Vec3::from_array(prim.normals[tri[0] as usize]);
            let face = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
            assert!(
                face.dot(n) > 0.0,
                "triangle winding disagrees with vertex normal: face {face} vs n {n}"
            );
            assert!((n.length() - 1.0).abs() < 1e-5, "non-unit normal");
        }
    }

    #[test]
    fn open_hall_has_floor_ceiling_perimeter_walls() {
        let mesh = generate(&doc_with(false));
        // 15 cells: 15 floor quads, 15 ceiling quads
        assert_eq!(mesh.floor.triangle_count(), 30);
        assert_eq!(mesh.ceiling.triangle_count(), 30);
        // perimeter = 2*(5+3) = 16 wall quads
        assert_eq!(mesh.wall.triangle_count(), 32);
        check_winding(&mesh.floor);
        check_winding(&mesh.wall);
        check_winding(&mesh.ceiling);
    }

    #[test]
    fn floor_normals_up_ceiling_down() {
        let mesh = generate(&doc_with(false));
        assert!(mesh.floor.normals.iter().all(|n| n[1] == 1.0));
        assert!(mesh.ceiling.normals.iter().all(|n| n[1] == -1.0));
        assert!(mesh.wall.normals.iter().all(|n| n[1] == 0.0));
    }

    #[test]
    fn door_adds_opening_geometry_on_both_sides() {
        let plain = generate(&doc_with(false));
        let doored = generate(&doc_with(true));
        // perimeter unchanged; doorway adds jambs+lintel per side (3 quads
        // ×2 sides) + 3 reveal quads ×2 sides = 12 quads = 24 triangles
        assert_eq!(
            doored.wall.triangle_count(),
            plain.wall.triangle_count() + 24
        );
        check_winding(&doored.wall);

        // the opening itself must be clear: no wall geometry crosses the
        // door centre at half door-height
        let centre = doored
            .wall
            .positions
            .iter()
            .filter(|p| (p[1] - 1.2).abs() < 0.4) // around mid door height
            .filter(|p| (p[0] - 1.0).abs() < 0.3 && p[2].abs() < 0.69)
            .count();
        assert_eq!(centre, 0, "vertices found inside the doorway opening");
    }

    #[test]
    fn second_storey_stacks_at_floor_height() {
        let mut doc = doc_with(false);
        let mut upper = crate::Floor::new(5, 3);
        upper.set(0, 0, Cell::Floor);
        doc.floors.push(upper);
        let mesh = generate(&doc);
        let max_y = mesh
            .wall
            .positions
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(max_y, doc.floor_height + doc.wall_height);
    }
}
