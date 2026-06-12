//! The island document: heightfield + masks + undo history + on-disk
//! project format. A project is a directory `<name>.wright/` holding
//! `project.toml` plus raw layer blobs — lossless (f32 heights, full-depth
//! masks), unlike the 16-bit export, so re-editing never degrades.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use wright_field::{Heightfield, Masks, Region};

pub struct IslandDoc {
    pub name: String,
    pub field: Heightfield,
    pub masks: Masks,
    /// Where this project lives on disk (None until first save).
    pub project_dir: Option<PathBuf>,
    pub dirty_since_save: bool,
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
}

/// One stroke's worth of reversible change. Strokes touch heights or masks;
/// we snapshot all mask layers in the region either way — a few KB.
pub struct UndoEntry {
    region: Region,
    before: LayerPatch,
    after: LayerPatch,
}

#[derive(Clone)]
struct LayerPatch {
    heights: Vec<f32>,
    rockness: Vec<u8>,
    autoshader: Vec<u8>,
    tint: Vec<[u8; 3]>,
}

fn snapshot_patch(field: &Heightfield, masks: &Masks, region: Region) -> LayerPatch {
    let res = field.resolution();
    let mut rockness = Vec::with_capacity(region.width() * region.height());
    let mut autoshader = Vec::with_capacity(region.width() * region.height());
    let mut tint = Vec::with_capacity(region.width() * region.height());
    for z in region.z0..=region.z1 {
        let row = z * res;
        rockness.extend_from_slice(&masks.rockness[row + region.x0..=row + region.x1]);
        autoshader.extend_from_slice(&masks.autoshader[row + region.x0..=row + region.x1]);
        tint.extend_from_slice(&masks.tint[row + region.x0..=row + region.x1]);
    }
    LayerPatch {
        heights: field.snapshot(region),
        rockness,
        autoshader,
        tint,
    }
}

fn restore_patch(field: &mut Heightfield, masks: &mut Masks, region: Region, patch: &LayerPatch) {
    field.restore(region, &patch.heights);
    let res = field.resolution();
    let w = region.width();
    for (i, z) in (region.z0..=region.z1).enumerate() {
        let row = z * res;
        masks.rockness[row + region.x0..=row + region.x1]
            .copy_from_slice(&patch.rockness[i * w..(i + 1) * w]);
        masks.autoshader[row + region.x0..=row + region.x1]
            .copy_from_slice(&patch.autoshader[i * w..(i + 1) * w]);
        masks.tint[row + region.x0..=row + region.x1]
            .copy_from_slice(&patch.tint[i * w..(i + 1) * w]);
    }
}

impl IslandDoc {
    pub fn new(name: &str, resolution: usize, world_size: f32, base_height: f32) -> Self {
        Self {
            name: name.to_string(),
            field: Heightfield::new(resolution, world_size, base_height),
            masks: Masks::new(resolution),
            project_dir: None,
            dirty_since_save: false,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Snapshot the stroke's dirty region *before* it ran (`pre` was taken at
    /// stroke begin over the whole field) and record an undo entry.
    pub fn commit_stroke(&mut self, pre_field: &Heightfield, pre_masks: &Masks, region: Region) {
        let before = snapshot_patch(pre_field, pre_masks, region);
        let after = snapshot_patch(&self.field, &self.masks, region);
        self.undo.push(UndoEntry {
            region,
            before,
            after,
        });
        const MAX_UNDO: usize = 256;
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty_since_save = true;
    }

    pub fn undo(&mut self) -> Option<Region> {
        let e = self.undo.pop()?;
        restore_patch(&mut self.field, &mut self.masks, e.region, &e.before);
        let region = e.region;
        self.redo.push(e);
        self.dirty_since_save = true;
        Some(region)
    }

    pub fn redo(&mut self) -> Option<Region> {
        let e = self.redo.pop()?;
        restore_patch(&mut self.field, &mut self.masks, e.region, &e.after);
        let region = e.region;
        self.undo.push(e);
        self.dirty_since_save = true;
        Some(region)
    }

    pub fn undo_depth(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }

    // ── project on disk ──────────────────────────────────────────────────

    pub fn save(&mut self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let res = self.field.resolution();
        let manifest = format!(
            "# wright island project\nname = \"{}\"\nresolution = {}\nworld_size = {}\n",
            self.name,
            res,
            self.field.world_size(),
        );
        fs::write(dir.join("project.toml"), manifest)?;

        let mut heights = Vec::with_capacity(res * res * 4);
        for &h in self.field.heights() {
            heights.extend_from_slice(&h.to_le_bytes());
        }
        fs::write(dir.join("heights.r32"), heights)?;
        fs::write(dir.join("rockness.u8"), &self.masks.rockness)?;
        fs::write(dir.join("autoshader.u8"), &self.masks.autoshader)?;
        let mut tint = Vec::with_capacity(res * res * 3);
        for t in &self.masks.tint {
            tint.extend_from_slice(t);
        }
        fs::write(dir.join("tint.rgb8"), tint)?;

        self.project_dir = Some(dir.to_path_buf());
        self.dirty_since_save = false;
        Ok(())
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let manifest: toml::Value = toml::from_str(
            &fs::read_to_string(dir.join("project.toml"))
                .with_context(|| format!("reading {}", dir.join("project.toml").display()))?,
        )?;
        let name = manifest
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("island")
            .to_string();
        let resolution = manifest
            .get("resolution")
            .and_then(|v| v.as_integer())
            .context("project.toml: missing resolution")? as usize;
        let world_size = manifest
            .get("world_size")
            .and_then(|v| v.as_float().or(v.as_integer().map(|i| i as f64)))
            .context("project.toml: missing world_size")? as f32;

        let raw = fs::read(dir.join("heights.r32"))?;
        anyhow::ensure!(
            raw.len() == resolution * resolution * 4,
            "heights.r32 wrong size: {} for resolution {resolution}",
            raw.len()
        );
        let heights: Vec<f32> = raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let field = Heightfield::from_heights(resolution, world_size, heights);

        let mut masks = Masks::new(resolution);
        let n = resolution * resolution;
        if let Ok(b) = fs::read(dir.join("rockness.u8"))
            && b.len() == n
        {
            masks.rockness = b;
        }
        if let Ok(b) = fs::read(dir.join("autoshader.u8"))
            && b.len() == n
        {
            masks.autoshader = b;
        }
        if let Ok(b) = fs::read(dir.join("tint.rgb8"))
            && b.len() == n * 3
        {
            masks.tint = b.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        }

        Ok(Self {
            name,
            field,
            masks,
            project_dir: Some(dir.to_path_buf()),
            dirty_since_save: false,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_field::{Brush, BrushKind, Stroke};

    #[test]
    fn undo_redo_roundtrip() {
        let mut doc = IslandDoc::new("t", 65, 64.0, 0.0);
        let pre_f = doc.field.clone();
        let pre_m = doc.masks.clone();
        let mut s = Stroke::new(
            Brush {
                kind: BrushKind::Raise,
                radius: 10.0,
                strength: 5.0,
                ..Default::default()
            },
            0.0,
        );
        s.apply(&mut doc.field, &mut doc.masks, 0.0, 0.0, 1.0);
        doc.commit_stroke(&pre_f, &pre_m, s.dirty.unwrap());

        let peak = doc.field.height_at(0.0, 0.0).unwrap();
        assert!(peak > 4.0);
        doc.undo().unwrap();
        assert!(doc.field.height_at(0.0, 0.0).unwrap().abs() < 1e-6);
        doc.redo().unwrap();
        assert!((doc.field.height_at(0.0, 0.0).unwrap() - peak).abs() < 1e-6);
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut doc = IslandDoc::new("isle", 33, 32.0, -2.0);
        doc.field.set(5, 7, 9.5);
        doc.masks.rockness[100] = 200;
        doc.masks.tint[3] = [10, 20, 30];
        doc.save(&dir.path().join("isle.wright")).unwrap();

        let loaded = IslandDoc::load(&dir.path().join("isle.wright")).unwrap();
        assert_eq!(loaded.name, "isle");
        assert_eq!(loaded.field.resolution(), 33);
        assert_eq!(loaded.field.get(5, 7), 9.5);
        assert_eq!(loaded.masks.rockness[100], 200);
        assert_eq!(loaded.masks.tint[3], [10, 20, 30]);
        assert_eq!(loaded.field.get(0, 0), -2.0);
    }
}
