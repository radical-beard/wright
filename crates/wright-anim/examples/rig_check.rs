//! Load a real glTF and print rig stats — sanity-checks the loader against
//! production models.
//!
//!     cargo run -p wright-anim --example rig_check -- <model.glb>

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: rig_check <model.glb>"))?;
    let rig = wright_anim::load_gltf(std::path::Path::new(&path))?;
    println!("{path}: {} bones", rig.bones.len());
    for clip in &rig.clips {
        println!(
            "  clip `{}` · {:.2}s · {} tracks",
            clip.name,
            clip.duration,
            clip.tracks.len()
        );
        // sample a pose mid-clip to prove the whole pipeline works
        let pose = clip.sample_pose(&rig, clip.duration * 0.5);
        let world = rig.world_matrices(&pose);
        let reach = world
            .iter()
            .map(|m| m.to_scale_rotation_translation().2.length())
            .fold(0.0f32, f32::max);
        println!("    mid-clip pose OK, max joint reach {reach:.2} m");
    }
    Ok(())
}
