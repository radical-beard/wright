//! Scene placement document → bestow scene TOML. The shape matches what
//! `bestow-ecs/src/template.rs::parse_scene` reads: `[[entities]]` entries
//! with `name` / `template` / `tags` and `components.transform` tables.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacedEntity {
    pub name: String,
    /// Template to instantiate; empty string = inline (components only).
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub position: [f32; 3],
    /// Rotation about +Y in degrees (the one rotation maps care about).
    #[serde(default)]
    pub yaw_deg: f32,
}

/// A placement project: entities arranged over an optional ground island.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SceneDoc {
    pub name: String,
    /// Path to the ground island's `.hgt.toml` (for display + snapping).
    #[serde(default)]
    pub ground: Option<String>,
    #[serde(default)]
    pub entities: Vec<PlacedEntity>,
}

impl SceneDoc {
    /// Render bestow `[[entities]]` blocks ready to paste into (or include
    /// as) a scene file.
    pub fn to_scene_toml(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "# `{}` — {} entities placed in wright.",
            self.name,
            self.entities.len()
        );
        for e in &self.entities {
            let _ = writeln!(s, "\n[[entities]]");
            if !e.name.is_empty() {
                let _ = writeln!(s, "name = \"{}\"", e.name);
            }
            if !e.template.is_empty() {
                let _ = writeln!(s, "template = \"{}\"", e.template);
            }
            if !e.tags.is_empty() {
                let tags: Vec<String> = e.tags.iter().map(|t| format!("\"{t}\"")).collect();
                let _ = writeln!(s, "tags = [{}]", tags.join(", "));
            }
            let _ = writeln!(s, "[entities.components.transform]");
            let _ = writeln!(
                s,
                "position = [{}, {}, {}]",
                e.position[0], e.position[1], e.position[2]
            );
            if e.yaw_deg.abs() > 1e-3 {
                // bestow transform.rotation = Euler XYZ radians (world.rs
                // local_transform), not a quaternion
                let _ = writeln!(
                    s,
                    "rotation = [0.0, {}, 0.0]  # yaw {}°",
                    e.yaw_deg.to_radians(),
                    e.yaw_deg
                );
            }
        }
        s
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let s = toml::to_string_pretty(self)?;
        std::fs::write(path, s).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let s =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Ok(toml::from_str(&s)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> SceneDoc {
        SceneDoc {
            name: "camp".into(),
            ground: Some("islands/wrightisle.hgt.toml".into()),
            entities: vec![
                PlacedEntity {
                    name: "chief".into(),
                    template: "goblin_chief".into(),
                    tags: vec!["enemy".into(), "boss".into()],
                    position: [10.0, 2.5, -4.0],
                    yaw_deg: 90.0,
                },
                PlacedEntity {
                    name: "".into(),
                    template: "campfire".into(),
                    tags: vec![],
                    position: [8.0, 2.0, -3.0],
                    yaw_deg: 0.0,
                },
            ],
        }
    }

    #[test]
    fn scene_toml_matches_bestow_shape() {
        let toml_src = doc().to_scene_toml();
        let v: toml::Value = toml::from_str(&toml_src).unwrap();
        let entities = v["entities"].as_array().unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0]["template"].as_str(), Some("goblin_chief"));
        assert_eq!(entities[0]["tags"].as_array().unwrap().len(), 2);
        let pos = entities[0]["components"]["transform"]["position"]
            .as_array()
            .unwrap();
        assert_eq!(pos[1].as_float(), Some(2.5));
        // yaw 90° → Euler XYZ radians [0, π/2, 0] (bestow's transform format)
        let rot = entities[0]["components"]["transform"]["rotation"]
            .as_array()
            .unwrap();
        assert_eq!(rot.len(), 3);
        assert!((rot[1].as_float().unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-3);
        // unnamed entity omits name, keeps transform
        assert!(entities[1].get("name").is_none());
        assert!(entities[1].get("rotation").is_none());
    }

    #[test]
    fn project_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("camp.wrightscene");
        doc().save(&path).unwrap();
        let back = SceneDoc::load(&path).unwrap();
        assert_eq!(back.entities.len(), 2);
        assert_eq!(back.entities[0].yaw_deg, 90.0);
        assert_eq!(back.ground.as_deref(), Some("islands/wrightisle.hgt.toml"));
    }
}
