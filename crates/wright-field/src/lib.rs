//! Heightfield island document: a square f32 height grid plus material
//! masks, mutated by falloff brushes. Pure logic — no GPU, no UI — so every
//! brush and the mesher are unit-testable. Mirrors what bestow consumes
//! (`[entities.components.terrain]`: 16-bit heightmap over
//! `[height_min, height_max]` meters) and what argh's Terrain Sculpt proved
//! out, with the things it lacked (undo, dirty-region meshing) built in.

mod brush;
mod mesh;
mod pick;

pub use brush::{Brush, BrushKind, Stroke};
pub use mesh::{CHUNK_QUADS, ChunkMesh, Mesher, Vertex};
pub use pick::raycast;

use glam::Vec2;

/// Square heightfield in world meters. Heights are absolute meters (not
/// normalized); export decides the min/max window. The grid covers
/// `[origin, origin + world_size]` on XZ, `resolution` samples per side,
/// so cell pitch is `world_size / (resolution - 1)` and corner samples sit
/// exactly on the island border — the same convention bestow's terrain
/// system uses when it bilinearly resamples the PNG.
#[derive(Clone)]
pub struct Heightfield {
    resolution: usize,
    world_size: f32,
    heights: Vec<f32>,
}

/// Painted material signal, one byte per height sample, matching the baked
/// island look bestow ships (`island_baked.slang`): R = rockness, G =
/// autoshader mask, plus an RGB tint layer exported as `<name>.color.png`.
#[derive(Clone)]
pub struct Masks {
    pub resolution: usize,
    /// 0 = first material (grass), 255 = second material (rock).
    pub rockness: Vec<u8>,
    /// 255 = let the shader pick by slope (Terrain3D autoshader behavior),
    /// 0 = the painted rockness wins. New islands start fully automatic.
    pub autoshader: Vec<u8>,
    /// RGB tint multiplied over the blended detail textures.
    pub tint: Vec<[u8; 3]>,
}

impl Masks {
    pub fn new(resolution: usize) -> Self {
        let n = resolution * resolution;
        Self {
            resolution,
            rockness: vec![0; n],
            autoshader: vec![255; n],
            tint: vec![[255, 255, 255]; n],
        }
    }
}

/// Inclusive sample-index rectangle dirtied by an edit, used to remesh only
/// affected chunks and to snapshot undo patches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x0: usize,
    pub z0: usize,
    pub x1: usize,
    pub z1: usize,
}

impl Region {
    pub fn union(self, other: Region) -> Region {
        Region {
            x0: self.x0.min(other.x0),
            z0: self.z0.min(other.z0),
            x1: self.x1.max(other.x1),
            z1: self.z1.max(other.z1),
        }
    }

    pub fn width(&self) -> usize {
        self.x1 - self.x0 + 1
    }

    pub fn height(&self) -> usize {
        self.z1 - self.z0 + 1
    }
}

impl Heightfield {
    /// `resolution` is samples per side (>= 2); `world_size` is the full XZ
    /// extent in meters. Starts flat at `base_height`.
    pub fn new(resolution: usize, world_size: f32, base_height: f32) -> Self {
        assert!(resolution >= 2, "heightfield needs at least 2x2 samples");
        Self {
            resolution,
            world_size,
            heights: vec![base_height; resolution * resolution],
        }
    }

    pub fn from_heights(resolution: usize, world_size: f32, heights: Vec<f32>) -> Self {
        assert_eq!(heights.len(), resolution * resolution);
        Self {
            resolution,
            world_size,
            heights,
        }
    }

    pub fn resolution(&self) -> usize {
        self.resolution
    }

    pub fn world_size(&self) -> f32 {
        self.world_size
    }

    /// Meters between adjacent samples.
    pub fn cell_size(&self) -> f32 {
        self.world_size / (self.resolution - 1) as f32
    }

    /// World X/Z of the minimum corner; the island is centred on the origin
    /// like bestow terrain entities (transform sits at the centre).
    pub fn world_origin(&self) -> Vec2 {
        Vec2::splat(-self.world_size * 0.5)
    }

    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    pub fn heights_mut(&mut self) -> &mut [f32] {
        &mut self.heights
    }

    pub fn get(&self, x: usize, z: usize) -> f32 {
        self.heights[z * self.resolution + x]
    }

    pub fn set(&mut self, x: usize, z: usize, h: f32) {
        self.heights[z * self.resolution + x] = h;
    }

    pub fn min_max(&self) -> (f32, f32) {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for &h in &self.heights {
            min = min.min(h);
            max = max.max(h);
        }
        (min, max)
    }

    /// World-space position of sample (x, z).
    pub fn sample_pos(&self, x: usize, z: usize) -> glam::Vec3 {
        let o = self.world_origin();
        let c = self.cell_size();
        glam::Vec3::new(o.x + x as f32 * c, self.get(x, z), o.y + z as f32 * c)
    }

    /// Bilinear height at an arbitrary world XZ; `None` outside the island.
    pub fn height_at(&self, wx: f32, wz: f32) -> Option<f32> {
        let o = self.world_origin();
        let c = self.cell_size();
        let fx = (wx - o.x) / c;
        let fz = (wz - o.y) / c;
        let max = (self.resolution - 1) as f32;
        if fx < 0.0 || fz < 0.0 || fx > max || fz > max {
            return None;
        }
        let x0 = (fx as usize).min(self.resolution - 2);
        let z0 = (fz as usize).min(self.resolution - 2);
        let (tx, tz) = (fx - x0 as f32, fz - z0 as f32);
        let s = |x: usize, z: usize| self.get(x, z);
        Some(
            s(x0, z0) * (1.0 - tx) * (1.0 - tz)
                + s(x0 + 1, z0) * tx * (1.0 - tz)
                + s(x0, z0 + 1) * (1.0 - tx) * tz
                + s(x0 + 1, z0 + 1) * tx * tz,
        )
    }

    /// Sample-index region touched by a world-space brush circle, clamped to
    /// the grid; `None` when the circle misses the island entirely.
    pub fn region_for_circle(&self, centre_x: f32, centre_z: f32, radius: f32) -> Option<Region> {
        let o = self.world_origin();
        let c = self.cell_size();
        let last = (self.resolution - 1) as f32;
        let fx0 = ((centre_x - radius - o.x) / c).floor();
        let fz0 = ((centre_z - radius - o.y) / c).floor();
        let fx1 = ((centre_x + radius - o.x) / c).ceil();
        let fz1 = ((centre_z + radius - o.y) / c).ceil();
        if fx1 < 0.0 || fz1 < 0.0 || fx0 > last || fz0 > last {
            return None;
        }
        Some(Region {
            x0: fx0.max(0.0) as usize,
            z0: fz0.max(0.0) as usize,
            x1: fx1.min(last) as usize,
            z1: fz1.min(last) as usize,
        })
    }

    /// Copy the heights inside `region` (row-major, region-local) — the undo
    /// snapshot primitive.
    pub fn snapshot(&self, region: Region) -> Vec<f32> {
        let mut out = Vec::with_capacity(region.width() * region.height());
        for z in region.z0..=region.z1 {
            let row = z * self.resolution;
            out.extend_from_slice(&self.heights[row + region.x0..=row + region.x1]);
        }
        out
    }

    /// Write a `snapshot` back into `region`.
    pub fn restore(&mut self, region: Region, patch: &[f32]) {
        assert_eq!(patch.len(), region.width() * region.height());
        let w = region.width();
        for (i, z) in (region.z0..=region.z1).enumerate() {
            let row = z * self.resolution;
            self.heights[row + region.x0..=row + region.x1]
                .copy_from_slice(&patch[i * w..(i + 1) * w]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_size_spans_world() {
        let f = Heightfield::new(5, 8.0, 0.0);
        assert_eq!(f.cell_size(), 2.0);
        let p = f.sample_pos(4, 4);
        assert_eq!((p.x, p.z), (4.0, 4.0)); // far corner = +size/2
    }

    #[test]
    fn bilinear_height_interpolates() {
        let mut f = Heightfield::new(2, 2.0, 0.0);
        f.set(1, 0, 1.0);
        f.set(0, 1, 1.0);
        // centre of the single cell averages all four corners
        assert!((f.height_at(0.0, 0.0).unwrap() - 0.5).abs() < 1e-6);
        assert!(f.height_at(2.0, 0.0).is_none());
    }

    #[test]
    fn snapshot_restore_roundtrip() {
        let mut f = Heightfield::new(8, 7.0, 0.0);
        let region = Region {
            x0: 2,
            z0: 3,
            x1: 5,
            z1: 6,
        };
        let before = f.snapshot(region);
        for z in 3..=6 {
            for x in 2..=5 {
                f.set(x, z, 9.0);
            }
        }
        f.restore(region, &before);
        assert!(f.heights().iter().all(|&h| h == 0.0));
    }

    #[test]
    fn circle_region_clamps_and_rejects() {
        let f = Heightfield::new(11, 10.0, 0.0); // origin -5..5, cell 1m
        let r = f.region_for_circle(4.0, 4.0, 3.0).unwrap();
        assert_eq!(
            r,
            Region {
                x0: 6,
                z0: 6,
                x1: 10,
                z1: 10
            }
        );
        assert!(f.region_for_circle(20.0, 0.0, 3.0).is_none());
    }
}
