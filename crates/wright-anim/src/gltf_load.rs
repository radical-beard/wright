//! glTF → Rig. Bones come from the first skin's joints (falling back to
//! the whole node hierarchy for unskinned files), re-ordered parent-first.
//! Per-property glTF channels (translation/rotation/scale each animate
//! separately) are merged into whole-pose tracks on a unified key timeline,
//! matching the track shape bestow bakes and wright previews.

use crate::rig::{Bone, Clip, LocalPose, Rig, Track};
use anyhow::{Context, Result};
use glam::{Quat, Vec3};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn load_gltf(path: &Path) -> Result<Rig> {
    let (doc, buffers, _images) =
        gltf::import(path).with_context(|| format!("importing {}", path.display()))?;

    // node index → parent node index
    let mut parent: Vec<Option<usize>> = vec![None; doc.nodes().len()];
    for node in doc.nodes() {
        for child in node.children() {
            parent[child.index()] = Some(node.index());
        }
    }

    // Bone set: first skin's joints, else every node.
    let joint_nodes: Vec<usize> = match doc.skins().next() {
        Some(skin) => skin.joints().map(|j| j.index()).collect(),
        None => doc.nodes().map(|n| n.index()).collect(),
    };
    let in_set: HashSet<usize> = joint_nodes.iter().copied().collect();

    // Nearest in-set ancestor = bone parent (skips non-joint scene nulls).
    let bone_parent_node = |node: usize| -> Option<usize> {
        let mut cur = parent[node];
        while let Some(p) = cur {
            if in_set.contains(&p) {
                return Some(p);
            }
            cur = parent[p];
        }
        None
    };

    // Parent-first ordering via DFS from in-set roots.
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut roots = Vec::new();
    for &n in &joint_nodes {
        match bone_parent_node(n) {
            Some(p) => children.entry(p).or_default().push(n),
            None => roots.push(n),
        }
    }
    let mut order = Vec::with_capacity(joint_nodes.len());
    let mut stack: Vec<usize> = roots;
    while let Some(n) = stack.pop() {
        order.push(n);
        if let Some(kids) = children.get(&n) {
            stack.extend(kids.iter().rev());
        }
    }
    anyhow::ensure!(
        order.len() == joint_nodes.len(),
        "joint hierarchy has a cycle or disconnected node"
    );

    let nodes: Vec<gltf::Node> = doc.nodes().collect();
    let mut node_to_bone: HashMap<usize, usize> = HashMap::new();
    let mut bones = Vec::with_capacity(order.len());
    for (bi, &ni) in order.iter().enumerate() {
        node_to_bone.insert(ni, bi);
        let node = &nodes[ni];
        let (t, r, s) = node.transform().decomposed();
        bones.push(Bone {
            name: node
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("bone{ni}")),
            parent: bone_parent_node(ni).map(|p| node_to_bone[&p]),
            rest_local: LocalPose {
                translation: Vec3::from_array(t),
                rotation: Quat::from_array(r),
                scale: Vec3::from_array(s),
            },
        });
    }

    // ── clips: merge per-property channels into whole-pose tracks ────────
    #[derive(Default)]
    struct Curves {
        t: Option<(Vec<f32>, Vec<Vec3>)>,
        r: Option<(Vec<f32>, Vec<Quat>)>,
        s: Option<(Vec<f32>, Vec<Vec3>)>,
    }

    let mut clips = Vec::new();
    for (ai, anim) in doc.animations().enumerate() {
        let mut per_bone: HashMap<usize, Curves> = HashMap::new();
        let mut duration = 0.0f32;

        for channel in anim.channels() {
            let Some(&bone) = node_to_bone.get(&channel.target().node().index()) else {
                continue; // animates a non-joint node (camera, prop)
            };
            let reader = channel.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
            let Some(times) = reader.read_inputs().map(|i| i.collect::<Vec<f32>>()) else {
                continue;
            };
            duration = duration.max(times.last().copied().unwrap_or(0.0));
            // CubicSpline output triplets [in-tangent, value, out-tangent]:
            // keep the value; preview interpolates linearly.
            let cubic =
                channel.sampler().interpolation() == gltf::animation::Interpolation::CubicSpline;
            let pick = |i: usize| if cubic { i * 3 + 1 } else { i };

            let curves = per_bone.entry(bone).or_default();
            match reader.read_outputs() {
                Some(gltf::animation::util::ReadOutputs::Translations(it)) => {
                    let vals: Vec<Vec3> = it.map(Vec3::from_array).collect();
                    let vals = (0..times.len()).map(|i| vals[pick(i)]).collect();
                    curves.t = Some((times, vals));
                }
                Some(gltf::animation::util::ReadOutputs::Rotations(it)) => {
                    let vals: Vec<Quat> = it.into_f32().map(Quat::from_array).collect();
                    let vals = (0..times.len()).map(|i| vals[pick(i)]).collect();
                    curves.r = Some((times, vals));
                }
                Some(gltf::animation::util::ReadOutputs::Scales(it)) => {
                    let vals: Vec<Vec3> = it.map(Vec3::from_array).collect();
                    let vals = (0..times.len()).map(|i| vals[pick(i)]).collect();
                    curves.s = Some((times, vals));
                }
                _ => {}
            }
        }

        let mut tracks = HashMap::new();
        for (bone, curves) in per_bone {
            // unified, sorted, deduped key timeline across the properties
            let mut times: Vec<f32> = Vec::new();
            for ts in [&curves.t, &curves.s].into_iter().flatten() {
                times.extend_from_slice(&ts.0);
            }
            if let Some((ts, _)) = &curves.r {
                times.extend_from_slice(ts);
            }
            times.sort_by(f32::total_cmp);
            times.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
            if times.is_empty() {
                continue;
            }

            let rest = bones[bone].rest_local;
            let poses = times
                .iter()
                .map(|&t| LocalPose {
                    translation: curves
                        .t
                        .as_ref()
                        .map_or(rest.translation, |(ts, vs)| sample_vec3(ts, vs, t)),
                    rotation: curves
                        .r
                        .as_ref()
                        .map_or(rest.rotation, |(ts, vs)| sample_quat(ts, vs, t)),
                    scale: curves
                        .s
                        .as_ref()
                        .map_or(rest.scale, |(ts, vs)| sample_vec3(ts, vs, t)),
                })
                .collect();
            tracks.insert(bones[bone].name.clone(), Track { times, poses });
        }

        clips.push(Clip {
            name: anim
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("clip{ai}")),
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

fn seg(times: &[f32], t: f32) -> (usize, usize, f32) {
    match times.binary_search_by(|k| k.total_cmp(&t)) {
        Ok(i) => (i, i, 0.0),
        Err(0) => (0, 0, 0.0),
        Err(i) if i >= times.len() => (times.len() - 1, times.len() - 1, 0.0),
        Err(i) => {
            let frac = (t - times[i - 1]) / (times[i] - times[i - 1]);
            (i - 1, i, frac.clamp(0.0, 1.0))
        }
    }
}

fn sample_vec3(times: &[f32], vals: &[Vec3], t: f32) -> Vec3 {
    let (a, b, f) = seg(times, t);
    vals[a].lerp(vals[b], f)
}

fn sample_quat(times: &[f32], vals: &[Quat], t: f32) -> Quat {
    let (a, b, f) = seg(times, t);
    vals[a].slerp(vals[b], f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Two-joint skin, one rotation channel on the child: 0 → 90° about Z
    /// over one second. Buffer written as a sibling .bin to skip base64.
    fn write_fixture(dir: &Path) -> std::path::PathBuf {
        let mut bin: Vec<u8> = Vec::new();
        for v in [0.0f32, 1.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        let half = std::f32::consts::FRAC_PI_4; // 90°/2
        for q in [[0.0f32, 0.0, 0.0, 1.0], [0.0, 0.0, half.sin(), half.cos()]] {
            for c in q {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        fs::write(dir.join("buf.bin"), &bin).unwrap();

        let json = r#"{
  "asset": {"version": "2.0"},
  "scene": 0,
  "scenes": [{"nodes": [0]}],
  "nodes": [
    {"name": "root", "children": [1]},
    {"name": "arm", "translation": [0, 1, 0]}
  ],
  "skins": [{"joints": [0, 1]}],
  "animations": [{
    "name": "spin",
    "samplers": [{"input": 0, "output": 1, "interpolation": "LINEAR"}],
    "channels": [{"sampler": 0, "target": {"node": 1, "path": "rotation"}}]
  }],
  "accessors": [
    {"bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0]},
    {"bufferView": 1, "componentType": 5126, "count": 2, "type": "VEC4"}
  ],
  "bufferViews": [
    {"buffer": 0, "byteOffset": 0, "byteLength": 8},
    {"buffer": 0, "byteOffset": 8, "byteLength": 32}
  ],
  "buffers": [{"uri": "buf.bin", "byteLength": 40}]
}"#;
        let path = dir.join("fixture.gltf");
        fs::write(&path, json).unwrap();
        path
    }

    #[test]
    fn loads_skeleton_and_clip() {
        let dir = tempfile::tempdir().unwrap();
        let rig = load_gltf(&write_fixture(dir.path())).unwrap();

        assert_eq!(rig.bones.len(), 2);
        assert_eq!(rig.bones[0].name, "root");
        assert_eq!(rig.bones[1].parent, Some(0));
        assert_eq!(
            rig.bones[1].rest_local.translation,
            Vec3::new(0.0, 1.0, 0.0)
        );

        assert_eq!(rig.clips.len(), 1);
        let clip = &rig.clips[0];
        assert_eq!(clip.name, "spin");
        assert!((clip.duration - 1.0).abs() < 1e-6);

        // halfway: 45° about Z; untracked translation holds rest
        let pose = clip.sample_pose(&rig, 0.5);
        let (axis, angle) = pose[1].rotation.to_axis_angle();
        assert!((angle - std::f32::consts::FRAC_PI_4).abs() < 1e-3);
        assert!((axis.z - 1.0).abs() < 1e-3);
        assert_eq!(pose[1].translation, Vec3::new(0.0, 1.0, 0.0));

        // world matrix of the arm tip reflects the hierarchy
        let world = rig.world_matrices(&pose);
        let tip = world[1].transform_point3(Vec3::new(1.0, 0.0, 0.0));
        assert!((tip - Vec3::new(0.7071, 1.7071, 0.0)).length() < 1e-3);
    }
}
