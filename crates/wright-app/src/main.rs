//! wright — the standalone editor that crafts visual resources and exports
//! them in bestow's formats. Bestow stays editor-free (its D-005: the game
//! is the visualizer); wright is where humans sculpt.

mod island;
mod modes;
mod render;
mod state;

use eframe::egui;
use island::IslandMode;
use modes::ModeId;
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
    active: ModeId,
}

impl WrightApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc
            .wgpu_render_state
            .clone()
            .expect("wright needs the wgpu backend (eframe Renderer::Wgpu)");
        let state = AppState::load();
        let island = IslandMode::new(render_state, &state);
        Self {
            state,
            island,
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
            other => modes::stub_panel(ui, other),
        }
    }
}
