//! Minimal glTF-binary (.glb) writer for the dungeon shell: one mesh, one
//! node, three primitives (floor / wall / ceiling) each with its own
//! material, POSITION + NORMAL + TEXCOORD_0 + u32 indices. Hand-rolled —
//! the format is a JSON chunk + BIN chunk behind a 12-byte header — and
//! verified in tests by reading the output back with the `gltf` crate.

use crate::meshgen::{Primitive, ShellMesh};
use anyhow::Result;
use serde_json::json;

const GLB_MAGIC: u32 = 0x4654_6C67; // "glTF"
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;
const F32: u32 = 5126; // GL component types
const U32: u32 = 5125;

struct Builder {
    bin: Vec<u8>,
    buffer_views: Vec<serde_json::Value>,
    accessors: Vec<serde_json::Value>,
}

impl Builder {
    fn pad4(&mut self) {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
    }

    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        self.pad4();
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.buffer_views.push(match target {
            Some(t) => {
                json!({"buffer": 0, "byteOffset": offset, "byteLength": bytes.len(), "target": t})
            }
            None => json!({"buffer": 0, "byteOffset": offset, "byteLength": bytes.len()}),
        });
        self.buffer_views.len() - 1
    }

    fn accessor_vec(&mut self, data: &[f32], dims: usize, with_bounds: bool) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|f| f.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34962)); // ARRAY_BUFFER
        let count = data.len() / dims;
        let ty = match dims {
            2 => "VEC2",
            3 => "VEC3",
            _ => unreachable!("only vec2/vec3 attributes"),
        };
        let mut acc = json!({
            "bufferView": view, "componentType": F32, "count": count, "type": ty
        });
        if with_bounds {
            let mut min = vec![f32::INFINITY; dims];
            let mut max = vec![f32::NEG_INFINITY; dims];
            for chunk in data.chunks_exact(dims) {
                for (i, &v) in chunk.iter().enumerate() {
                    min[i] = min[i].min(v);
                    max[i] = max[i].max(v);
                }
            }
            acc["min"] = json!(min);
            acc["max"] = json!(max);
        }
        self.accessors.push(acc);
        self.accessors.len() - 1
    }

    fn accessor_indices(&mut self, data: &[u32]) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|i| i.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34963)); // ELEMENT_ARRAY_BUFFER
        self.accessors.push(json!({
            "bufferView": view, "componentType": U32, "count": data.len(), "type": "SCALAR"
        }));
        self.accessors.len() - 1
    }
}

/// Serialize the shell as a .glb byte blob.
pub fn glb_bytes(name: &str, mesh: &ShellMesh) -> Result<Vec<u8>> {
    let mut b = Builder {
        bin: Vec::new(),
        buffer_views: Vec::new(),
        accessors: Vec::new(),
    };

    // (primitive, material name, baseColor)
    let parts: [(&Primitive, &str, [f32; 4]); 3] = [
        (&mesh.floor, "floor", [0.45, 0.40, 0.34, 1.0]),
        (&mesh.wall, "wall", [0.52, 0.50, 0.48, 1.0]),
        (&mesh.ceiling, "ceiling", [0.30, 0.29, 0.28, 1.0]),
    ];

    let mut primitives = Vec::new();
    let mut materials = Vec::new();
    for (prim, mat_name, color) in parts {
        if prim.indices.is_empty() {
            continue;
        }
        let flat = |v: &[[f32; 3]]| v.iter().flatten().copied().collect::<Vec<f32>>();
        let flat2 = |v: &[[f32; 2]]| v.iter().flatten().copied().collect::<Vec<f32>>();
        let pos = b.accessor_vec(&flat(&prim.positions), 3, true);
        let nrm = b.accessor_vec(&flat(&prim.normals), 3, false);
        let uv = b.accessor_vec(&flat2(&prim.uvs), 2, false);
        let idx = b.accessor_indices(&prim.indices);
        let material = materials.len();
        materials.push(json!({
            "name": mat_name,
            "pbrMetallicRoughness": {
                "baseColorFactor": color,
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0
            }
        }));
        primitives.push(json!({
            "attributes": {"POSITION": pos, "NORMAL": nrm, "TEXCOORD_0": uv},
            "indices": idx,
            "material": material
        }));
    }
    anyhow::ensure!(!primitives.is_empty(), "dungeon shell is empty");

    b.pad4();
    let root = json!({
        "asset": {"version": "2.0", "generator": "wright"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": name}],
        "meshes": [{"name": name, "primitives": primitives}],
        "materials": materials,
        "accessors": b.accessors,
        "bufferViews": b.buffer_views,
        "buffers": [{"byteLength": b.bin.len()}],
    });

    let mut json_bytes = serde_json::to_vec(&root)?;
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }

    let total = 12 + 8 + json_bytes.len() + 8 + b.bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&GLB_MAGIC.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_JSON.to_le_bytes());
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(b.bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
    out.extend_from_slice(&b.bin);
    Ok(out)
}

pub fn write_glb(path: &std::path::Path, name: &str, mesh: &ShellMesh) -> Result<()> {
    let bytes = glb_bytes(name, mesh)?;
    let tmp = path.with_extension("tmp~");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cell, DungeonDoc, meshgen};

    fn small_doc() -> DungeonDoc {
        let mut doc = DungeonDoc::new("g", 3, 3);
        for z in 0..3 {
            for x in 0..3 {
                doc.floors[0].set(x, z, Cell::Floor);
            }
        }
        doc
    }

    #[test]
    fn glb_roundtrips_through_gltf_crate() {
        let doc = small_doc();
        let mesh = meshgen::generate(&doc);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("g.glb");
        write_glb(&path, "g", &mesh).unwrap();

        // strict readback: gltf::import validates the whole file
        let (gdoc, buffers, _) = gltf::import(&path).unwrap();
        let gmesh = gdoc.meshes().next().unwrap();
        assert_eq!(gmesh.primitives().count(), 3);

        let mut total_tris = 0;
        for prim in gmesh.primitives() {
            let reader = prim.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
            let pos: Vec<[f32; 3]> = reader.read_positions().unwrap().collect();
            let nrm: Vec<[f32; 3]> = reader.read_normals().unwrap().collect();
            let uv: Vec<[f32; 2]> = reader.read_tex_coords(0).unwrap().into_f32().collect();
            let idx: Vec<u32> = reader.read_indices().unwrap().into_u32().collect();
            assert_eq!(pos.len(), nrm.len());
            assert_eq!(pos.len(), uv.len());
            assert!(idx.iter().all(|&i| (i as usize) < pos.len()));
            total_tris += idx.len() / 3;
        }
        assert_eq!(total_tris, mesh.triangle_count());
        assert_eq!(gdoc.materials().count(), 3);

        // POSITION bounds present and sane (3x3 cells of 2m centred: ±3)
        let prim = gmesh.primitives().next().unwrap();
        let bounds = prim.bounding_box();
        assert_eq!(bounds.min[0], -3.0);
        assert_eq!(bounds.max[0], 3.0);
    }

    #[test]
    fn empty_shell_errors() {
        let doc = DungeonDoc::new("empty", 3, 3);
        let mesh = meshgen::generate(&doc);
        assert!(glb_bytes("empty", &mesh).is_err());
    }
}
