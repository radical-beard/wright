//! FBX → Rig via ufbx — the same crate and conventions bestow itself uses
//! (D-012: FBX is first-class), so what wright previews matches what the
//! engine plays. Clips bake every bone's local transform at a fixed rate
//! with `evaluate_transform`; no FBX curve semantics leak past this module.

use crate::rig::{Bone, Clip, LocalPose, Rig, Track};
use anyhow::{Context, Result};
use glam::{Quat, Vec3};
use std::collections::HashMap;
use std::path::Path;

const BAKE_RATE: f32 = 30.0;

fn load_opts() -> ufbx::LoadOpts<'static> {
    ufbx::LoadOpts {
        // bestow's convention: right-handed Y-up, meters
        target_axes: ufbx::CoordinateAxes::right_handed_y_up(),
        target_unit_meters: 1.0,
        space_conversion: ufbx::SpaceConversion::AdjustTransforms,
        ..Default::default()
    }
}

fn pose_of(t: &ufbx::Transform) -> LocalPose {
    LocalPose {
        translation: Vec3::new(
            t.translation.x as f32,
            t.translation.y as f32,
            t.translation.z as f32,
        ),
        rotation: Quat::from_xyzw(
            t.rotation.x as f32,
            t.rotation.y as f32,
            t.rotation.z as f32,
            t.rotation.w as f32,
        ),
        scale: Vec3::new(t.scale.x as f32, t.scale.y as f32, t.scale.z as f32),
    }
}

pub fn load_fbx(path: &Path) -> Result<Rig> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let scene = ufbx::load_memory(&bytes, load_opts())
        .map_err(|e| anyhow::anyhow!("{}: {e:?}", path.display()))?;

    // Bone set: nodes carrying a bone attribute; unrigged files fall back
    // to the whole node hierarchy (same policy as the glTF loader).
    let has_bones = scene.nodes.iter().any(|n| n.bone.is_some());
    let is_bone = |n: &ufbx::Node| -> bool {
        if has_bones {
            n.bone.is_some()
        } else {
            !n.is_root
        }
    };

    // Parent-first order: scene.nodes is hierarchy-ordered in ufbx, so a
    // single pass with nearest-bone-ancestor lookup suffices.
    let mut bones: Vec<Bone> = Vec::new();
    let mut by_node_id: HashMap<u32, usize> = HashMap::new();
    for node in &scene.nodes {
        if !is_bone(node) {
            continue;
        }
        let parent = {
            let mut cur = node.parent.as_ref();
            let mut found = None;
            while let Some(p) = cur {
                if let Some(&bi) = by_node_id.get(&p.element.element_id) {
                    found = Some(bi);
                    break;
                }
                cur = p.parent.as_ref();
            }
            found
        };
        by_node_id.insert(node.element.element_id, bones.len());
        bones.push(Bone {
            name: node.element.name.to_string(),
            parent,
            rest_local: pose_of(&node.local_transform),
        });
    }
    anyhow::ensure!(!bones.is_empty(), "{}: no nodes", path.display());

    // One clip per animation stack (FBX "takes"), baked at a fixed rate.
    let mut clips = Vec::new();
    for stack in &scene.anim_stacks {
        let begin = stack.time_begin;
        let duration = (stack.time_end - stack.time_begin).max(0.0) as f32;
        if duration <= 0.0 {
            continue;
        }
        let frame_count = ((duration * BAKE_RATE).ceil() as usize + 1).max(2);
        let times: Vec<f32> = (0..frame_count)
            .map(|f| (f as f32 / BAKE_RATE).min(duration))
            .collect();

        let mut tracks = HashMap::new();
        for node in &scene.nodes {
            if !by_node_id.contains_key(&node.element.element_id) {
                continue;
            }
            let poses: Vec<LocalPose> = times
                .iter()
                .map(|&t| {
                    pose_of(&ufbx::evaluate_transform(
                        &stack.anim,
                        node,
                        begin + t as f64,
                    ))
                })
                .collect();
            tracks.insert(
                node.element.name.to_string(),
                Track {
                    times: times.clone(),
                    poses,
                },
            );
        }
        clips.push(Clip {
            name: stack.element.name.to_string(),
            duration,
            tracks,
        });
    }

    let by_name = bones
        .iter()
        .enumerate()
        .map(|(i, b)| (b.name.clone(), i))
        .collect();
    Ok(Rig {
        bones,
        by_name,
        clips,
    })
}

/// Load a model by extension: `.fbx` via ufbx, `.glb`/`.gltf` via the glTF
/// loader.
pub fn load_model(path: &Path) -> Result<Rig> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("fbx") => load_fbx(path),
        Some("glb") | Some("gltf") => crate::load_gltf(path),
        other => anyhow::bail!("unsupported model format {other:?} (use .glb, .gltf, or .fbx)"),
    }
}
