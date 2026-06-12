//! Animation rig document: a skeleton + its clips loaded from glTF, plus
//! the authored metadata wright exists to produce — named sockets on bones,
//! event tags at clip times, and clip sections (splitting one source clip
//! into combo segments with early-out points). Mirrors bestow's animation
//! model (`bestow-anim`: per-bone-name `LocalPose` tracks, lerp/slerp
//! sampling) so the preview matches engine playback.

mod gltf_load;
mod rig;

pub use gltf_load::load_gltf;
pub use rig::{Bone, Clip, LocalPose, Rig, Track};

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// A named attachment point on a bone: where weapons, effects, and child
/// entities hook on (bestow templates: `attach_socket = "hand_r"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Socket {
    pub name: String,
    pub bone: String,
    /// Local offset from the bone, meters.
    pub offset: [f32; 3],
    /// Local rotation offset, quaternion [x, y, z, w].
    pub rotation: [f32; 4],
}

/// A tagged moment in a clip — bestow fires these as gameplay events at the
/// exact sample time (`[[animation.clips.events]]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTag {
    pub clip: String,
    pub name: String,
    /// Seconds from clip start.
    pub time: f32,
    /// Free-form payload keys (string values keep the TOML trivial).
    #[serde(default)]
    pub payload: Vec<(String, String)>,
}

/// A named slice of a source clip (`[[animation.clips.sections]]`): the
/// combo-splitting primitive. A section that `can_end` is a point where the
/// player may bail out of the combo chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub clip: String,
    pub name: String,
    pub start: f32,
    pub end: f32,
    /// Player may end the combo when this section finishes.
    pub can_end: bool,
}

/// Everything wright authors on top of a model file. Saved in the project
/// and exported as bestow TOML next to the model.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnimMeta {
    /// Asset path of the source model as the game references it.
    pub model: String,
    #[serde(default)]
    pub sockets: Vec<Socket>,
    #[serde(default)]
    pub events: Vec<EventTag>,
    #[serde(default)]
    pub sections: Vec<Section>,
}

impl AnimMeta {
    /// Render the bestow animation metadata TOML (documented schema:
    /// `[[animation.clips]]` with nested events/sections, plus
    /// `[[animation.sockets]]`). Stable order: clips alphabetically, events
    /// and sections by time.
    pub fn to_bestow_toml(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "# Animation metadata for `{}` (authored in wright).",
            self.model
        );
        let _ = writeln!(s, "[animation]");
        let _ = writeln!(s, "model = \"{}\"", self.model);

        for socket in &self.sockets {
            let _ = writeln!(s, "\n[[animation.sockets]]");
            let _ = writeln!(s, "name = \"{}\"", socket.name);
            let _ = writeln!(s, "bone = \"{}\"", socket.bone);
            let _ = writeln!(
                s,
                "offset = [{}, {}, {}]",
                socket.offset[0], socket.offset[1], socket.offset[2]
            );
            let _ = writeln!(
                s,
                "rotation = [{}, {}, {}, {}]",
                socket.rotation[0], socket.rotation[1], socket.rotation[2], socket.rotation[3]
            );
        }

        let mut clips: Vec<&str> = self
            .events
            .iter()
            .map(|e| e.clip.as_str())
            .chain(self.sections.iter().map(|c| c.clip.as_str()))
            .collect();
        clips.sort_unstable();
        clips.dedup();

        for clip in clips {
            let _ = writeln!(s, "\n[[animation.clips]]");
            let _ = writeln!(s, "name = \"{clip}\"");

            let mut events: Vec<&EventTag> =
                self.events.iter().filter(|e| e.clip == clip).collect();
            events.sort_by(|a, b| a.time.total_cmp(&b.time));
            for e in events {
                let _ = writeln!(s, "\n[[animation.clips.events]]");
                let _ = writeln!(s, "name = \"{}\"", e.name);
                let _ = writeln!(s, "time = {}", e.time);
                if !e.payload.is_empty() {
                    let kv: Vec<String> = e
                        .payload
                        .iter()
                        .map(|(k, v)| format!("{k} = \"{v}\""))
                        .collect();
                    let _ = writeln!(s, "payload = {{ {} }}", kv.join(", "));
                }
            }

            let mut sections: Vec<&Section> =
                self.sections.iter().filter(|c| c.clip == clip).collect();
            sections.sort_by(|a, b| a.start.total_cmp(&b.start));
            for c in sections {
                let _ = writeln!(s, "\n[[animation.clips.sections]]");
                let _ = writeln!(s, "name = \"{}\"", c.name);
                let _ = writeln!(s, "start = {}", c.start);
                let _ = writeln!(s, "end = {}", c.end);
                if c.can_end {
                    let _ = writeln!(s, "can_end = true");
                }
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bestow_toml_is_valid_and_ordered() {
        let meta = AnimMeta {
            model: "models/hero.glb".into(),
            sockets: vec![Socket {
                name: "weapon".into(),
                bone: "hand_r".into(),
                offset: [0.05, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            }],
            events: vec![
                EventTag {
                    clip: "attack".into(),
                    name: "hit".into(),
                    time: 0.42,
                    payload: vec![("power".into(), "heavy".into())],
                },
                EventTag {
                    clip: "attack".into(),
                    name: "windup_end".into(),
                    time: 0.2,
                    payload: vec![],
                },
            ],
            sections: vec![
                Section {
                    clip: "attack".into(),
                    name: "swing_1".into(),
                    start: 0.0,
                    end: 0.6,
                    can_end: true,
                },
                Section {
                    clip: "attack".into(),
                    name: "swing_2".into(),
                    start: 0.6,
                    end: 1.4,
                    can_end: false,
                },
            ],
        };
        let toml_src = meta.to_bestow_toml();
        // must parse as TOML
        let v: toml::Value = toml::from_str(&toml_src).unwrap();
        let clips = v["animation"]["clips"].as_array().unwrap();
        assert_eq!(clips.len(), 1);
        let events = clips[0]["events"].as_array().unwrap();
        // sorted by time: windup_end first
        assert_eq!(events[0]["name"].as_str(), Some("windup_end"));
        assert_eq!(events[1]["payload"]["power"].as_str(), Some("heavy"));
        let sections = clips[0]["sections"].as_array().unwrap();
        assert_eq!(sections[0]["can_end"].as_bool(), Some(true));
        assert_eq!(
            v["animation"]["sockets"].as_array().unwrap()[0]["bone"].as_str(),
            Some("hand_r")
        );
    }

    #[test]
    fn roundtrips_through_serde() {
        let meta = AnimMeta {
            model: "m.glb".into(),
            sockets: vec![],
            events: vec![EventTag {
                clip: "run".into(),
                name: "footstep".into(),
                time: 0.31,
                payload: vec![("foot".into(), "left".into())],
            }],
            sections: vec![],
        };
        let s = toml::to_string(&meta).unwrap();
        let back: AnimMeta = toml::from_str(&s).unwrap();
        assert_eq!(back.events[0].name, "footstep");
        assert_eq!(back.events[0].payload[0].1, "left");
    }
}
