//! Build a small two-room dungeon and export the self-contained bestow
//! folder — the headless e2e fixture and a scriptable export example.
//!
//!     cargo run -p wright-dungeon --example export_demo -- <dungeons_dir>

use wright_dungeon::{Cell, Door, DoorKind, DungeonDoc, DungeonEntity};

fn main() -> anyhow::Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/wright-dungeons".into());

    let mut doc = DungeonDoc::new("gloomhold", 11, 5);
    let f = &mut doc.floors[0];
    // entry hall (4x3), corridor, boss room (4x5)
    for z in 1..4 {
        for x in 0..4 {
            f.set(x, z, Cell::Floor);
        }
    }
    f.set(4, 2, Cell::Floor);
    f.set(5, 2, Cell::Floor);
    f.set(6, 2, Cell::Floor);
    for z in 0..5 {
        for x in 7..11 {
            f.set(x, z, Cell::Floor);
        }
    }
    doc.doors.push(Door {
        name: "boss_door".into(),
        floor: 0,
        a: (6, 2),
        b: (7, 2),
        kind: DoorKind::Locked {
            key: "boss_key".into(),
        },
    });
    doc.doors.push(Door {
        name: "hall_arch".into(),
        floor: 0,
        a: (3, 2),
        b: (4, 2),
        kind: DoorKind::Open,
    });
    doc.entities.push(DungeonEntity {
        name: "spawn".into(),
        template: String::new(),
        tags: vec!["player_spawn".into()],
        position: doc.cell_center(0, 1, 2),
        yaw_deg: 90.0,
    });
    doc.entities.push(DungeonEntity {
        name: "boss_key_chest".into(),
        template: String::new(),
        tags: vec!["chest".into(), "key.boss_key".into()],
        position: doc.cell_center(0, 2, 1),
        yaw_deg: 0.0,
    });

    let report = wright_dungeon::export::export_dungeon(
        &doc,
        std::path::Path::new(&out),
        "assets/dungeons",
    )?;
    println!(
        "exported {} files, {} shell triangles → {}",
        report.files.len(),
        report.triangle_count,
        report.scene_rel
    );
    for w in &report.warnings {
        println!("warning: {w}");
    }
    Ok(())
}
