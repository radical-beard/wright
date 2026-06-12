//! Headless sculpt + export: proves the whole pipeline without the GUI and
//! doubles as a scriptable export path.
//!
//!     cargo run -p wright-bestow --example sculpt_export -- <out_dir> [name]

use wright_bestow::{ExportOptions, export_island};
use wright_field::{Brush, BrushKind, Heightfield, Masks, Stroke};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let out_dir = args.next().unwrap_or_else(|| "/tmp/wright-export".into());
    let name = args.next().unwrap_or_else(|| "wrightisle".into());

    let mut field = Heightfield::new(257, 256.0, -8.0);
    let mut masks = Masks::new(257);

    let mut stroke = |kind: BrushKind, radius: f32, strength: f32, x: f32, z: f32, secs: f32| {
        let anchor = field.height_at(x, z).unwrap_or(0.0);
        let mut s = Stroke::new(
            Brush {
                kind,
                radius,
                strength,
                ..Default::default()
            },
            anchor,
        );
        // simulate held-brush frames
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        while t < secs {
            s.apply(&mut field, &mut masks, x, z, dt);
            t += dt;
        }
    };

    // a central mound breaking the waterline, two foothills, a smoothed bay
    stroke(BrushKind::Raise, 90.0, 14.0, 0.0, 0.0, 1.6);
    stroke(BrushKind::Raise, 45.0, 12.0, 60.0, 30.0, 1.0);
    stroke(BrushKind::Raise, 38.0, 12.0, -55.0, -42.0, 0.9);
    stroke(BrushKind::Noise, 70.0, 6.0, 0.0, 0.0, 0.5);
    stroke(BrushKind::Smooth, 50.0, 10.0, -20.0, 40.0, 0.6);
    stroke(BrushKind::PaintRock, 30.0, 3.0, 60.0, 30.0, 0.8);

    let report = export_island(&field, &masks, &ExportOptions::new(&name, &out_dir))?;
    println!(
        "exported {} files to {out_dir} (h [{:.1}, {:.1}])",
        report.files.len(),
        report.height_min,
        report.height_max
    );
    println!("\n{}", report.entity_toml);
    Ok(())
}
