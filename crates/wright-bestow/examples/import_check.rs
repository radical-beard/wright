//! Import an existing bestow island and print its stats — sanity-checks the
//! re-edit path against real game data.
//!
//!     cargo run -p wright-bestow --example import_check -- <path/to/name.hgt.toml>

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: import_check <name.hgt.toml>"))?;
    let (field, masks, name) = wright_bestow::import_island(std::path::Path::new(&path))?;
    let (hmin, hmax) = field.min_max();
    let painted = masks.autoshader.iter().filter(|&&a| a < 128).count();
    let rocky = masks.rockness.iter().filter(|&&r| r > 128).count();
    println!(
        "{name}: {res}x{res} over {size:.0} m, heights [{hmin:.1}, {hmax:.1}] m, \
         {painted} painted samples, {rocky} rocky samples",
        res = field.resolution(),
        size = field.world_size(),
    );
    Ok(())
}
