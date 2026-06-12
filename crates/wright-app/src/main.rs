//! wright — the standalone editor that crafts visual resources and exports
//! them in bestow's formats. Bestow stays editor-free (its D-005: the game
//! is the visualizer); wright is where humans sculpt.

mod anim;
mod dungeon;
mod island;
mod modes;
mod placement;
mod render;
mod state;

use anim::AnimMode;
use dungeon::DungeonMode;
use eframe::egui;
use island::IslandMode;
use modes::ModeId;
use placement::PlacementMode;
use state::AppState;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1480.0, 920.0])
            .with_title("wright"),
        ..Default::default()
    };
    eframe::run_native(
        "wright",
        options,
        Box::new(|cc| Ok(Box::new(WrightApp::new(cc)))),
    )
}

struct WrightApp {
    state: AppState,
    island: IslandMode,
    anim: AnimMode,
    dungeon: DungeonMode,
    placement: PlacementMode,
    active: ModeId,
}

impl WrightApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc
            .wgpu_render_state
            .clone()
            .expect("wright needs the wgpu backend (eframe Renderer::Wgpu)");
        let state = AppState::load();
        let island = IslandMode::new(render_state.clone(), &state);
        let anim = AnimMode::new(render_state.clone());
        let dungeon = DungeonMode::new(render_state.clone());
        let placement = PlacementMode::new(render_state);
        Self {
            state,
            island,
            anim,
            dungeon,
            placement,
            active: ModeId::Island,
        }
    }
}

impl eframe::App for WrightApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("modes").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("wright").strong());
                ui.separator();
                for mode in ModeId::ALL {
                    if ui
                        .selectable_label(self.active == mode, mode.label())
                        .clicked()
                    {
                        self.active = mode;
                    }
                }
            });
        });

        match self.active {
            ModeId::Island => self.island.update(ui, &mut self.state),
            ModeId::Animation => self.anim.update(ui, &mut self.state),
            ModeId::Dungeon => self.dungeon.update(ui, &mut self.state),
            ModeId::Placement => self.placement.update(ui, &mut self.state),
        }
    }
}
