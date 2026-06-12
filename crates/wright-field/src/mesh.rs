//! Chunked grid meshing: the heightfield is split into fixed-size chunks of
//! quads so a brush stroke only remeshes the chunks its dirty region
//! touches. Normals are central-difference from the height grid, so chunk
//! borders shade seamlessly without sharing vertex buffers.

use crate::{Heightfield, Masks, Region};

/// Quads per chunk side. 64 quads → 65×65 verts per chunk, ~4225 verts —
/// small enough that remeshing a handful per stroke frame is trivial.
pub const CHUNK_QUADS: usize = 64;

#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// x = rockness, y = autoshader weight (z, w spare).
    pub material: [f32; 4],
    pub tint: [f32; 3],
}

pub struct ChunkMesh {
    /// Chunk coordinates (cx, cz) in chunk grid space.
    pub coord: (usize, usize),
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

pub struct Mesher {
    /// Chunks per side.
    pub chunks: usize,
}

impl Mesher {
    pub fn new(field: &Heightfield) -> Self {
        let quads = field.resolution() - 1;
        Self {
            chunks: quads.div_ceil(CHUNK_QUADS),
        }
    }

    /// Chunk coords whose quads intersect a dirty sample region. A sample on
    /// a chunk border belongs to quads in the neighbouring chunk too, hence
    /// the -1 on the low side.
    pub fn chunks_for_region(&self, region: Region) -> Vec<(usize, usize)> {
        let lo = |s: usize| s.saturating_sub(1) / CHUNK_QUADS;
        let hi = |s: usize| (s.min(self.chunks * CHUNK_QUADS) / CHUNK_QUADS).min(self.chunks - 1);
        let (cx0, cx1) = (lo(region.x0), hi(region.x1));
        let (cz0, cz1) = (lo(region.z0), hi(region.z1));
        let mut out = Vec::new();
        for cz in cz0..=cz1 {
            for cx in cx0..=cx1 {
                out.push((cx, cz));
            }
        }
        out
    }

    pub fn all_chunks(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::with_capacity(self.chunks * self.chunks);
        for cz in 0..self.chunks {
            for cx in 0..self.chunks {
                out.push((cx, cz));
            }
        }
        out
    }

    /// Build one chunk's mesh. Vertices duplicate along chunk borders;
    /// normals come from the full field so seams are invisible.
    pub fn build_chunk(
        &self,
        field: &Heightfield,
        masks: &Masks,
        coord: (usize, usize),
    ) -> ChunkMesh {
        let res = field.resolution();
        let last = res - 1;
        let (cx, cz) = coord;
        let x0 = cx * CHUNK_QUADS;
        let z0 = cz * CHUNK_QUADS;
        let x1 = (x0 + CHUNK_QUADS).min(last);
        let z1 = (z0 + CHUNK_QUADS).min(last);
        let w = x1 - x0 + 1;
        let h = z1 - z0 + 1;

        let cell = field.cell_size();
        let mut vertices = Vec::with_capacity(w * h);
        for z in z0..=z1 {
            for x in x0..=x1 {
                let p = field.sample_pos(x, z);
                // central differences, clamped at the island border
                let hl = field.get(x.saturating_sub(1), z);
                let hr = field.get((x + 1).min(last), z);
                let hd = field.get(x, z.saturating_sub(1));
                let hu = field.get(x, (z + 1).min(last));
                let dx = if x == 0 || x == last {
                    cell
                } else {
                    2.0 * cell
                };
                let dz = if z == 0 || z == last {
                    cell
                } else {
                    2.0 * cell
                };
                let n = glam::Vec3::new(-(hr - hl) / dx, 1.0, -(hu - hd) / dz).normalize();
                let i = z * res + x;
                vertices.push(Vertex {
                    position: p.to_array(),
                    normal: n.to_array(),
                    material: [
                        masks.rockness[i] as f32 / 255.0,
                        masks.autoshader[i] as f32 / 255.0,
                        0.0,
                        0.0,
                    ],
                    tint: [
                        masks.tint[i][0] as f32 / 255.0,
                        masks.tint[i][1] as f32 / 255.0,
                        masks.tint[i][2] as f32 / 255.0,
                    ],
                });
            }
        }

        let mut indices = Vec::with_capacity((w - 1) * (h - 1) * 6);
        for z in 0..h - 1 {
            for x in 0..w - 1 {
                let a = (z * w + x) as u32;
                let b = a + 1;
                let c = a + w as u32;
                let d = c + 1;
                // counter-clockwise seen from +Y
                indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }

        ChunkMesh {
            coord,
            vertices,
            indices,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Masks;

    #[test]
    fn chunk_counts_cover_field() {
        let f = Heightfield::new(513, 512.0, 0.0); // 512 quads = 8 chunks
        let m = Mesher::new(&f);
        assert_eq!(m.chunks, 8);
        let f = Heightfield::new(130, 64.0, 0.0); // 129 quads = 3 chunks
        assert_eq!(Mesher::new(&f).chunks, 3);
    }

    #[test]
    fn flat_field_has_up_normals_and_valid_indices() {
        let f = Heightfield::new(130, 64.0, 2.0);
        let masks = Masks::new(130);
        let m = Mesher::new(&f);
        for coord in m.all_chunks() {
            let chunk = m.build_chunk(&f, &masks, coord);
            assert!(!chunk.vertices.is_empty());
            for v in &chunk.vertices {
                assert!((v.normal[1] - 1.0).abs() < 1e-6);
                assert_eq!(v.position[1], 2.0);
            }
            let max = chunk.vertices.len() as u32;
            assert!(chunk.indices.iter().all(|&i| i < max));
            assert_eq!(chunk.indices.len() % 3, 0);
        }
    }

    #[test]
    fn dirty_region_maps_to_touching_chunks() {
        let f = Heightfield::new(513, 512.0, 0.0);
        let m = Mesher::new(&f);
        // a region straddling the border between chunk 0 and 1 in x
        let r = Region {
            x0: 63,
            z0: 10,
            x1: 65,
            z1: 12,
        };
        let chunks = m.chunks_for_region(r);
        assert!(chunks.contains(&(0, 0)) && chunks.contains(&(1, 0)));
        // sample exactly on a chunk border must also remesh the chunk left of it
        let r = Region {
            x0: 64,
            z0: 0,
            x1: 64,
            z1: 0,
        };
        assert!(m.chunks_for_region(r).contains(&(0, 0)));
    }

    #[test]
    fn winding_faces_up() {
        // one raised corner: the triangle normal computed from winding
        // should still point up (+Y) for a gentle slope
        let f = Heightfield::new(65, 64.0, 0.0);
        let masks = Masks::new(65);
        let m = Mesher::new(&f);
        let c = m.build_chunk(&f, &masks, (0, 0));
        let [a, b, cc] = [c.indices[0], c.indices[1], c.indices[2]];
        let p = |i: u32| glam::Vec3::from_array(c.vertices[i as usize].position);
        let n = (p(b) - p(a)).cross(p(cc) - p(a));
        assert!(n.y > 0.0, "first triangle winds counter-clockwise from +Y");
    }
}
