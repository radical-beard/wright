//! Runtime rig model: bone hierarchy + sampled clips, the same shape as
//! bestow-anim (per-bone-name tracks of TRS poses, linear/slerp sampling)
//! so wright's preview is what the engine plays.

use glam::{Mat4, Quat, Vec3};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalPose {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl LocalPose {
    pub const IDENTITY: LocalPose = LocalPose {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn to_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    pub fn lerp(&self, other: &LocalPose, t: f32) -> LocalPose {
        LocalPose {
            translation: self.translation.lerp(other.translation, t),
            rotation: self.rotation.slerp(other.rotation, t),
            scale: self.scale.lerp(other.scale, t),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    /// Parent index; bones are topologically ordered (parent before child).
    pub parent: Option<usize>,
    pub rest_local: LocalPose,
}

/// One animated channel: keyframe times plus poses for a single bone.
/// Times are strictly increasing; sampling clamps at the ends.
#[derive(Debug, Clone)]
pub struct Track {
    pub times: Vec<f32>,
    pub poses: Vec<LocalPose>,
}

impl Track {
    pub fn sample(&self, t: f32) -> LocalPose {
        match self.times.binary_search_by(|k| k.total_cmp(&t)) {
            Ok(i) => self.poses[i],
            Err(0) => self.poses[0],
            Err(i) if i >= self.times.len() => *self.poses.last().unwrap(),
            Err(i) => {
                let (t0, t1) = (self.times[i - 1], self.times[i]);
                let frac = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
                self.poses[i - 1].lerp(&self.poses[i], frac)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Clip {
    pub name: String,
    pub duration: f32,
    /// bone name → track.
    pub tracks: HashMap<String, Track>,
}

impl Clip {
    /// Sample every bone of `rig` at time `t` (seconds, clamped or wrapped
    /// by the caller); bones without a track hold their rest pose.
    pub fn sample_pose(&self, rig: &Rig, t: f32) -> Vec<LocalPose> {
        rig.bones
            .iter()
            .map(|b| {
                self.tracks
                    .get(&b.name)
                    .map_or(b.rest_local, |track| track.sample(t))
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Rig {
    pub bones: Vec<Bone>,
    pub by_name: HashMap<String, usize>,
    pub clips: Vec<Clip>,
}

impl Rig {
    /// Compose bone-local poses into world (model-space) matrices.
    pub fn world_matrices(&self, locals: &[LocalPose]) -> Vec<Mat4> {
        debug_assert_eq!(locals.len(), self.bones.len());
        let mut world: Vec<Mat4> = Vec::with_capacity(self.bones.len());
        for (i, bone) in self.bones.iter().enumerate() {
            let local = locals[i].to_matrix();
            let m = match bone.parent {
                Some(p) if p < i => world[p] * local,
                _ => local,
            };
            world.push(m);
        }
        world
    }

    pub fn rest_pose(&self) -> Vec<LocalPose> {
        self.bones.iter().map(|b| b.rest_local).collect()
    }

    /// Model-space transform of one bone under `locals` — what a socket
    /// gizmo anchors to.
    pub fn bone_world(&self, locals: &[LocalPose], bone: usize) -> Mat4 {
        self.world_matrices(locals)[bone]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_bone_rig() -> Rig {
        let bones = vec![
            Bone {
                name: "root".into(),
                parent: None,
                rest_local: LocalPose::IDENTITY,
            },
            Bone {
                name: "child".into(),
                parent: Some(0),
                rest_local: LocalPose {
                    translation: Vec3::new(0.0, 1.0, 0.0),
                    ..LocalPose::IDENTITY
                },
            },
        ];
        let by_name = bones
            .iter()
            .enumerate()
            .map(|(i, b)| (b.name.clone(), i))
            .collect();
        Rig {
            bones,
            by_name,
            clips: vec![],
        }
    }

    #[test]
    fn track_sampling_interpolates_and_clamps() {
        let track = Track {
            times: vec![0.0, 1.0],
            poses: vec![
                LocalPose::IDENTITY,
                LocalPose {
                    translation: Vec3::new(2.0, 0.0, 0.0),
                    ..LocalPose::IDENTITY
                },
            ],
        };
        assert_eq!(track.sample(0.5).translation.x, 1.0);
        assert_eq!(track.sample(-1.0).translation.x, 0.0);
        assert_eq!(track.sample(9.0).translation.x, 2.0);
    }

    #[test]
    fn hierarchy_composes() {
        let rig = two_bone_rig();
        let mut locals = rig.rest_pose();
        locals[0].translation.x = 5.0;
        let world = rig.world_matrices(&locals);
        let child_pos = world[1].to_scale_rotation_translation().2;
        assert_eq!(child_pos, Vec3::new(5.0, 1.0, 0.0));
    }

    #[test]
    fn clip_falls_back_to_rest() {
        let rig = two_bone_rig();
        let clip = Clip {
            name: "wave".into(),
            duration: 1.0,
            tracks: HashMap::from([(
                "root".into(),
                Track {
                    times: vec![0.0, 1.0],
                    poses: vec![
                        LocalPose::IDENTITY,
                        LocalPose {
                            translation: Vec3::new(0.0, 0.0, 3.0),
                            ..LocalPose::IDENTITY
                        },
                    ],
                },
            )]),
        };
        let pose = clip.sample_pose(&rig, 1.0);
        assert_eq!(pose[0].translation.z, 3.0);
        assert_eq!(pose[1].translation.y, 1.0, "untracked bone keeps rest");
    }
}
