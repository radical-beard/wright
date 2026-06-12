//! Falloff brushes over the heightfield and masks. Every op reports the
//! dirtied [`Region`] so the app can remesh chunks and capture undo patches.
//! Falloff follows argh's Terrain Sculpt: smoothstep from 1 at the brush
//! centre to 0 at the rim, sharpened or softened by a falloff exponent.

use crate::{Heightfield, Masks, Region};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrushKind {
    /// Push the surface up (clay add).
    Raise,
    /// Dig down (clay carve).
    Lower,
    /// Blend toward the brush's anchor height — terraces and plateaus.
    Flatten,
    /// Relax toward the neighbourhood mean.
    Smooth,
    /// Deterministic value noise for natural roughness.
    Noise,
    /// Paint rockness up (toward rock) — disables autoshader where painted.
    PaintRock,
    /// Paint rockness down (toward grass) — disables autoshader where painted.
    PaintGrass,
    /// Restore the slope-driven autoshader (erase painted material).
    PaintAuto,
    /// Paint the RGB tint layer.
    Tint,
}

impl BrushKind {
    pub fn is_material(self) -> bool {
        matches!(
            self,
            BrushKind::PaintRock | BrushKind::PaintGrass | BrushKind::PaintAuto | BrushKind::Tint
        )
    }

    pub const ALL: [BrushKind; 9] = [
        BrushKind::Raise,
        BrushKind::Lower,
        BrushKind::Flatten,
        BrushKind::Smooth,
        BrushKind::Noise,
        BrushKind::PaintRock,
        BrushKind::PaintGrass,
        BrushKind::PaintAuto,
        BrushKind::Tint,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BrushKind::Raise => "Raise",
            BrushKind::Lower => "Lower",
            BrushKind::Flatten => "Flatten",
            BrushKind::Smooth => "Smooth",
            BrushKind::Noise => "Noise",
            BrushKind::PaintRock => "Paint rock",
            BrushKind::PaintGrass => "Paint grass",
            BrushKind::PaintAuto => "Paint auto",
            BrushKind::Tint => "Tint",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Brush {
    pub kind: BrushKind,
    /// World meters.
    pub radius: f32,
    /// Meters of height change per second of brushing at full falloff
    /// (sculpt brushes), or mask units per second (paint brushes).
    pub strength: f32,
    /// Falloff exponent: 1 = smoothstep, <1 wider plateau, >1 sharper spike.
    pub falloff: f32,
    /// Tint color for [`BrushKind::Tint`].
    pub tint: [u8; 3],
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            kind: BrushKind::Raise,
            radius: 12.0,
            strength: 14.0,
            falloff: 1.0,
            tint: [255, 255, 255],
        }
    }
}

/// A stroke applies one brush over consecutive frames, holding state the
/// individual applications need (the flatten anchor height) and the union
/// of dirtied regions for the undo patch.
pub struct Stroke {
    pub brush: Brush,
    /// Height sampled where the stroke began; Flatten levels toward it.
    pub anchor_height: f32,
    pub dirty: Option<Region>,
}

impl Stroke {
    pub fn new(brush: Brush, anchor_height: f32) -> Self {
        Self {
            brush,
            anchor_height,
            dirty: None,
        }
    }

    /// Apply one frame of brushing at world (x, z). `dt` scales strength so
    /// sculpt speed is framerate-independent. Returns the region dirtied by
    /// this application (already folded into `self.dirty`).
    pub fn apply(
        &mut self,
        field: &mut Heightfield,
        masks: &mut Masks,
        cx: f32,
        cz: f32,
        dt: f32,
    ) -> Option<Region> {
        let region = field.region_for_circle(cx, cz, self.brush.radius)?;
        let b = self.brush;
        let amount = b.strength * dt;

        // Smooth needs the pre-pass heights so the blur doesn't smear
        // directionally as the loop scans.
        let before = matches!(b.kind, BrushKind::Smooth).then(|| field.snapshot(region));

        let res = field.resolution();
        let falloff_at = |x: usize, z: usize, field: &Heightfield| -> f32 {
            let p = field.sample_pos(x, z);
            let d = ((p.x - cx).powi(2) + (p.z - cz).powi(2)).sqrt();
            if d >= b.radius {
                return 0.0;
            }
            let t = 1.0 - d / b.radius;
            let s = t * t * (3.0 - 2.0 * t); // smoothstep
            s.powf(b.falloff.max(0.01))
        };

        for z in region.z0..=region.z1 {
            for x in region.x0..=region.x1 {
                let w = falloff_at(x, z, field);
                if w <= 0.0 {
                    continue;
                }
                match b.kind {
                    BrushKind::Raise => {
                        let h = field.get(x, z);
                        field.set(x, z, h + amount * w);
                    }
                    BrushKind::Lower => {
                        let h = field.get(x, z);
                        field.set(x, z, h - amount * w);
                    }
                    BrushKind::Flatten => {
                        let h = field.get(x, z);
                        let t = (amount * w).clamp(0.0, 1.0);
                        field.set(x, z, h + (self.anchor_height - h) * t);
                    }
                    BrushKind::Smooth => {
                        let snap = before.as_ref().unwrap();
                        let local = |xx: isize, zz: isize| -> f32 {
                            let xx = xx.clamp(region.x0 as isize, region.x1 as isize) as usize;
                            let zz = zz.clamp(region.z0 as isize, region.z1 as isize) as usize;
                            snap[(zz - region.z0) * region.width() + (xx - region.x0)]
                        };
                        let (xi, zi) = (x as isize, z as isize);
                        let mean = (local(xi - 1, zi)
                            + local(xi + 1, zi)
                            + local(xi, zi - 1)
                            + local(xi, zi + 1)
                            + local(xi, zi))
                            / 5.0;
                        let h = field.get(x, z);
                        let t = (amount * w).clamp(0.0, 1.0);
                        field.set(x, z, h + (mean - h) * t);
                    }
                    BrushKind::Noise => {
                        let h = field.get(x, z);
                        field.set(x, z, h + amount * w * value_noise(x as u32, z as u32));
                    }
                    BrushKind::PaintRock | BrushKind::PaintGrass => {
                        let i = z * res + x;
                        let delta = (b.strength * dt * 255.0 * w) as i32;
                        let v = masks.rockness[i] as i32;
                        let v = if matches!(b.kind, BrushKind::PaintRock) {
                            v + delta
                        } else {
                            v - delta
                        };
                        masks.rockness[i] = v.clamp(0, 255) as u8;
                        // painting an explicit material overrides autoshader
                        let a = masks.autoshader[i] as i32 - delta.max(1);
                        masks.autoshader[i] = a.clamp(0, 255) as u8;
                    }
                    BrushKind::PaintAuto => {
                        let i = z * res + x;
                        let delta = (b.strength * dt * 255.0 * w) as i32;
                        let a = masks.autoshader[i] as i32 + delta;
                        masks.autoshader[i] = a.clamp(0, 255) as u8;
                    }
                    BrushKind::Tint => {
                        let i = z * res + x;
                        let t = (b.strength * dt * w).clamp(0.0, 1.0);
                        for c in 0..3 {
                            let cur = masks.tint[i][c] as f32;
                            masks.tint[i][c] = (cur + (b.tint[c] as f32 - cur) * t) as u8;
                        }
                    }
                }
            }
        }

        self.dirty = Some(self.dirty.map_or(region, |d| d.union(region)));
        Some(region)
    }
}

/// Cheap deterministic per-sample noise in [-1, 1] (hash of the grid index).
/// Determinism keeps strokes reproducible — same input, same island.
fn value_noise(x: u32, z: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B9) ^ z.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Heightfield, Masks) {
        (Heightfield::new(65, 64.0, 0.0), Masks::new(65))
    }

    #[test]
    fn raise_peaks_at_centre_and_respects_radius() {
        let (mut f, mut m) = setup();
        let mut s = Stroke::new(
            Brush {
                kind: BrushKind::Raise,
                radius: 8.0,
                strength: 10.0,
                ..Default::default()
            },
            0.0,
        );
        s.apply(&mut f, &mut m, 0.0, 0.0, 1.0).unwrap();
        let centre = f.height_at(0.0, 0.0).unwrap();
        assert!(
            (centre - 10.0).abs() < 0.5,
            "centre ≈ strength*dt, got {centre}"
        );
        assert!(
            f.height_at(0.0, 9.0).unwrap().abs() < 1e-4,
            "outside radius untouched"
        );
        let mid = f.height_at(0.0, 4.0).unwrap();
        assert!(mid > 0.0 && mid < centre, "falloff between centre and rim");
    }

    #[test]
    fn lower_mirrors_raise() {
        let (mut f, mut m) = setup();
        let brush = Brush {
            kind: BrushKind::Lower,
            radius: 8.0,
            strength: 10.0,
            ..Default::default()
        };
        Stroke::new(brush, 0.0).apply(&mut f, &mut m, 0.0, 0.0, 1.0);
        assert!(f.height_at(0.0, 0.0).unwrap() < -9.0);
    }

    #[test]
    fn flatten_converges_to_anchor() {
        let (mut f, mut m) = setup();
        for h in f.heights_mut() {
            *h = 5.0;
        }
        let brush = Brush {
            kind: BrushKind::Flatten,
            radius: 8.0,
            strength: 10.0,
            ..Default::default()
        };
        let mut s = Stroke::new(brush, 2.0);
        for _ in 0..60 {
            s.apply(&mut f, &mut m, 0.0, 0.0, 1.0 / 30.0);
        }
        assert!((f.height_at(0.0, 0.0).unwrap() - 2.0).abs() < 0.05);
        assert!((f.height_at(0.0, 20.0).unwrap() - 5.0).abs() < 1e-4);
    }

    #[test]
    fn smooth_reduces_spike() {
        let (mut f, mut m) = setup();
        f.set(32, 32, 10.0);
        let brush = Brush {
            kind: BrushKind::Smooth,
            radius: 6.0,
            strength: 20.0,
            ..Default::default()
        };
        let mut s = Stroke::new(brush, 0.0);
        for _ in 0..30 {
            s.apply(&mut f, &mut m, 0.0, 0.0, 1.0 / 30.0);
        }
        assert!(f.get(32, 32) < 5.0, "spike relaxed, got {}", f.get(32, 32));
    }

    #[test]
    fn paint_rock_sets_mask_and_kills_autoshader() {
        let (mut f, mut m) = setup();
        let brush = Brush {
            kind: BrushKind::PaintRock,
            radius: 6.0,
            strength: 4.0,
            ..Default::default()
        };
        let mut s = Stroke::new(brush, 0.0);
        for _ in 0..30 {
            s.apply(&mut f, &mut m, 0.0, 0.0, 1.0 / 30.0);
        }
        let i = 32 * 65 + 32;
        assert!(m.rockness[i] > 200);
        assert!(m.autoshader[i] < 50);
        assert_eq!(m.rockness[0], 0, "corner untouched");
    }

    #[test]
    fn stroke_accumulates_dirty_union() {
        let (mut f, mut m) = setup();
        let brush = Brush {
            kind: BrushKind::Raise,
            radius: 4.0,
            strength: 1.0,
            ..Default::default()
        };
        let mut s = Stroke::new(brush, 0.0);
        s.apply(&mut f, &mut m, -20.0, -20.0, 0.016);
        s.apply(&mut f, &mut m, 20.0, 20.0, 0.016);
        let d = s.dirty.unwrap();
        assert!(d.width() > 30 && d.height() > 30);
    }

    #[test]
    fn noise_is_deterministic() {
        assert_eq!(value_noise(7, 9), value_noise(7, 9));
        assert!(value_noise(7, 9) >= -1.0 && value_noise(7, 9) <= 1.0);
    }
}
