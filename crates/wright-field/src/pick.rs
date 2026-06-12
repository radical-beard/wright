//! CPU ray ↔ heightfield intersection for brush picking. Fixed-step march
//! at half a cell, refined by bisection — exact enough that the brush ring
//! sits visually on the surface, and far simpler than a DDA over the grid.

use crate::Heightfield;
use glam::Vec3;

/// First hit of `origin + t*dir` with the heightfield surface, or the hit
/// with the y=0 water plane outside the island (so brushes can pull new
/// land up from the sea like argh's empty-space sculpting). Returns world
/// position. `None` when the ray escapes without hitting either.
pub fn raycast(field: &Heightfield, origin: Vec3, dir: Vec3) -> Option<Vec3> {
    let dir = dir.normalize();
    let step = field.cell_size() * 0.5;
    let max_t = field.world_size() * 4.0 + origin.y.abs() * 4.0;

    let height_or_sea = |x: f32, z: f32| field.height_at(x, z).unwrap_or(0.0);

    let mut t = 0.0;
    let mut prev_t = 0.0;
    let mut prev_above = origin.y - height_or_sea(origin.x, origin.z) > 0.0;
    while t < max_t {
        t += step;
        let p = origin + dir * t;
        let above = p.y - height_or_sea(p.x, p.z) > 0.0;
        if above != prev_above {
            // bisect between prev_t and t
            let (mut lo, mut hi) = (prev_t, t);
            for _ in 0..24 {
                let mid = 0.5 * (lo + hi);
                let q = origin + dir * mid;
                if (q.y - height_or_sea(q.x, q.z) > 0.0) == prev_above {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let hit = origin + dir * (0.5 * (lo + hi));
            return Some(hit);
        }
        prev_t = t;
        prev_above = above;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_ray_hits_flat_field() {
        let f = Heightfield::new(65, 64.0, 3.0);
        let hit = raycast(&f, Vec3::new(5.0, 50.0, -7.0), Vec3::NEG_Y).unwrap();
        assert!((hit.y - 3.0).abs() < 0.01);
        assert!((hit.x - 5.0).abs() < 0.01 && (hit.z + 7.0).abs() < 0.01);
    }

    #[test]
    fn diagonal_ray_hits_hill_side() {
        let mut f = Heightfield::new(65, 64.0, 0.0);
        // a hill in the middle
        for z in 28..=36 {
            for x in 28..=36 {
                f.set(x, z, 8.0);
            }
        }
        let hit = raycast(&f, Vec3::new(-30.0, 20.0, 0.0), Vec3::new(1.0, -0.4, 0.0)).unwrap();
        assert!(hit.y > -0.5, "hit at or above sea level, got {hit}");
    }

    #[test]
    fn ray_outside_island_hits_sea_plane() {
        let f = Heightfield::new(65, 64.0, 5.0);
        let hit = raycast(&f, Vec3::new(100.0, 30.0, 0.0), Vec3::new(0.2, -1.0, 0.0)).unwrap();
        assert!(hit.y.abs() < 0.05, "sea-plane hit, got {hit}");
    }

    #[test]
    fn skyward_ray_misses() {
        let f = Heightfield::new(65, 64.0, 0.0);
        assert!(raycast(&f, Vec3::new(0.0, 10.0, 0.0), Vec3::Y).is_none());
    }
}
