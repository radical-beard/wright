//! Editor modes. Island is fully implemented; the rest are visible,
//! honest placeholders that document what each will do — the roadmap lives
//! in the app, not just in a doc nobody opens.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModeId {
    Island,
    Animation,
    Dungeon,
    Placement,
}

impl ModeId {
    pub const ALL: [ModeId; 4] = [
        ModeId::Island,
        ModeId::Animation,
        ModeId::Dungeon,
        ModeId::Placement,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ModeId::Island => "🏝 Island",
            ModeId::Animation => "🦴 Animation",
            ModeId::Dungeon => "🏰 Dungeon",
            ModeId::Placement => "📍 Placement",
        }
    }
}

pub fn stub_panel(root: &mut eframe::egui::Ui, mode: ModeId) {
    use eframe::egui;
    egui::CentralPanel::default().show_inside(root, |ui| {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.heading(format!("{} — designed, not yet built", mode.label()));
            ui.add_space(12.0);
            let body: &[&str] = match mode {
                ModeId::Dungeon => &[
                    "Planned (see ROADMAP.md):",
                    "• Room/corridor layout on a grid with prefab room pieces",
                    "• Door/connection graph, lock-and-key annotations",
                    "• Per-room entity spawn sets",
                    "Exports: scene TOML (entities + includes) bestow loads directly.",
                ],
                ModeId::Placement => &[
                    "Planned (see ROADMAP.md):",
                    "• Place entity templates on exported islands/dungeons",
                    "• Spawners with tags, counts, radii",
                    "• Snap-to-terrain, alignment, scatter brush",
                    "Exports: [[entities]] scene TOML blocks.",
                ],
                // both fully implemented — never routed here
                ModeId::Island | ModeId::Animation => &[],
            };
            for line in body {
                ui.label(*line);
            }
        });
    });
}
