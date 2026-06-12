//! Island mode: sculpt + paint a heightfield island and export it straight
//! into a bestow game's assets. Viewport is the offscreen wgpu scene;
//! brushes run on hover position picked by CPU raycast.

pub mod doc;

use crate::render::camera::OrbitCamera;
use crate::render::scene::{SceneParams, SceneRenderer};
use crate::state::AppState;
use doc::IslandDoc;
use eframe::egui::{self, Color32, Key, PointerButton, RichText, Sense};
use eframe::egui_wgpu::RenderState;
use wright_field::{Brush, BrushKind, Heightfield, Masks, Stroke};

pub struct IslandMode {
    doc: IslandDoc,
    camera: OrbitCamera,
    scene: SceneRenderer,
    brush: Brush,
    /// Live stroke plus pre-stroke snapshots for the undo entry.
    stroke: Option<ActiveStroke>,
    hover_hit: Option<glam::Vec3>,
    time: f32,
    new_dialog: Option<NewIslandDialog>,
    export: ExportUi,
    status: String,
    needs_full_upload: bool,
}

struct ActiveStroke {
    stroke: Stroke,
    pre_field: Heightfield,
    pre_masks: Masks,
}

struct NewIslandDialog {
    name: String,
    resolution_idx: usize,
    world_size: f32,
    base_height: f32,
}

const RESOLUTIONS: [usize; 4] = [129, 257, 513, 1025];

#[derive(Default)]
struct ExportUi {
    asset_prefix: String,
    last_report: Option<wright_bestow::ExportReport>,
    error: Option<String>,
}

impl IslandMode {
    pub fn new(render_state: RenderState, state: &AppState) -> Self {
        let doc = state
            .last_project
            .as_ref()
            .and_then(|p| IslandDoc::load(p).ok())
            .unwrap_or_else(|| IslandDoc::new("island", 513, 512.0, -6.0));
        let camera = OrbitCamera::for_island(doc.field.world_size());
        let scene = SceneRenderer::new(render_state);
        Self {
            doc,
            camera,
            scene,
            brush: Brush::default(),
            stroke: None,
            hover_hit: None,
            time: 0.0,
            new_dialog: None,
            export: ExportUi {
                asset_prefix: "assets/islands".into(),
                ..Default::default()
            },
            status:
                "Sculpt with LMB. Orbit: RMB-drag · Pan: MMB or Shift+RMB · Zoom: scroll · F: frame"
                    .into(),
            needs_full_upload: true,
        }
    }

    pub fn update(&mut self, root: &mut egui::Ui, state: &mut AppState) {
        let ctx = root.ctx().clone();
        self.time += ctx.input(|i| i.stable_dt).min(0.1);
        if self.needs_full_upload {
            self.scene.upload_all(&self.doc.field, &self.doc.masks);
            self.needs_full_upload = false;
        }

        self.shortcuts(&ctx, state);
        self.tools_panel(root);
        self.island_panel(root, state);
        self.status_bar(root);
        self.viewport(root);
        self.new_island_dialog(&ctx);

        // keep water + strokes animating
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }

    // ── input & panels ────────────────────────────────────────────────────

    fn shortcuts(&mut self, ctx: &egui::Context, state: &mut AppState) {
        let undo = ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                Key::Z,
            ))
        });
        let redo = ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                Key::Z,
            ))
        });
        let save = ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                Key::S,
            ))
        });
        if undo && let Some(region) = self.doc.undo() {
            self.scene
                .upload_region(&self.doc.field, &self.doc.masks, region);
        }
        if redo && let Some(region) = self.doc.redo() {
            self.scene
                .upload_region(&self.doc.field, &self.doc.masks, region);
        }
        if save {
            self.save_project(state, false);
        }
        // brush hotkeys while not typing
        if !ctx.egui_wants_keyboard_input() {
            ctx.input(|i| {
                for (n, kind) in BrushKind::ALL.iter().enumerate() {
                    let key = [
                        Key::Num1,
                        Key::Num2,
                        Key::Num3,
                        Key::Num4,
                        Key::Num5,
                        Key::Num6,
                        Key::Num7,
                        Key::Num8,
                        Key::Num9,
                    ][n];
                    if i.key_pressed(key) {
                        self.brush.kind = *kind;
                    }
                }
                if i.key_pressed(Key::OpenBracket) {
                    self.brush.radius = (self.brush.radius / 1.2).max(0.5);
                }
                if i.key_pressed(Key::CloseBracket) {
                    self.brush.radius = (self.brush.radius * 1.2).min(200.0);
                }
            });
        }
    }

    fn tools_panel(&mut self, root: &mut egui::Ui) {
        egui::Panel::left("tools")
            .default_size(190.0)
            .show_inside(root, |ui| {
                ui.add_space(4.0);
                ui.heading("Sculpt");
                for (n, kind) in BrushKind::ALL.iter().enumerate() {
                    if *kind == BrushKind::PaintRock {
                        ui.add_space(8.0);
                        ui.heading("Paint");
                    }
                    let selected = self.brush.kind == *kind;
                    let label = format!("{}  {}", n + 1, kind.label());
                    if ui.selectable_label(selected, label).clicked() {
                        self.brush.kind = *kind;
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                ui.label("Radius (m)   [ ]");
                ui.add(egui::Slider::new(&mut self.brush.radius, 0.5..=200.0).logarithmic(true));
                ui.label("Strength");
                ui.add(egui::Slider::new(&mut self.brush.strength, 0.5..=100.0).logarithmic(true));
                ui.label("Falloff");
                ui.add(egui::Slider::new(&mut self.brush.falloff, 0.2..=4.0));
                if self.brush.kind == BrushKind::Tint {
                    ui.label("Tint color");
                    let mut c = Color32::from_rgb(
                        self.brush.tint[0],
                        self.brush.tint[1],
                        self.brush.tint[2],
                    );
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        self.brush.tint = [c.r(), c.g(), c.b()];
                    }
                }

                ui.add_space(10.0);
                ui.separator();
                let (undo_n, redo_n) = self.doc.undo_depth();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(undo_n > 0, egui::Button::new("⟲ Undo"))
                        .clicked()
                        && let Some(r) = self.doc.undo()
                    {
                        self.scene
                            .upload_region(&self.doc.field, &self.doc.masks, r);
                    }
                    if ui
                        .add_enabled(redo_n > 0, egui::Button::new("⟳ Redo"))
                        .clicked()
                        && let Some(r) = self.doc.redo()
                    {
                        self.scene
                            .upload_region(&self.doc.field, &self.doc.masks, r);
                    }
                });
                ui.label(RichText::new(format!("history {undo_n} ⟲ / {redo_n} ⟳")).weak());
            });
    }

    fn island_panel(&mut self, root: &mut egui::Ui, state: &mut AppState) {
        egui::Panel::right("island").default_size(240.0).show_inside(root, |ui| {
            ui.add_space(4.0);
            ui.heading("Project");
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut self.doc.name);
            });
            let (hmin, hmax) = self.doc.field.min_max();
            ui.label(format!(
                "{0}×{0} · {1:.0} m · h {2:.1}..{3:.1} m",
                self.doc.field.resolution(),
                self.doc.field.world_size(),
                hmin,
                hmax
            ));
            ui.horizontal(|ui| {
                if ui.button("New…").clicked() {
                    self.new_dialog = Some(NewIslandDialog {
                        name: "island".into(),
                        resolution_idx: 2,
                        world_size: 512.0,
                        base_height: -6.0,
                    });
                }
                if ui.button("Open…").clicked() {
                    self.open_project(state);
                }
                let save_label = if self.doc.dirty_since_save { "Save*" } else { "Save" };
                if ui.button(save_label).clicked() {
                    self.save_project(state, false);
                }
                if ui.button("Save as…").clicked() {
                    self.save_project(state, true);
                }
            });
            if let Some(dir) = &self.doc.project_dir {
                ui.label(RichText::new(dir.display().to_string()).weak().small());
            }

            ui.add_space(12.0);
            ui.separator();
            ui.heading("Export to bestow");
            ui.label("Writes <name>.hgt.png / .hgt.toml / .ctl.png /\n.color.png + UUID sidecars + entity snippet.");
            ui.horizontal(|ui| {
                ui.label("Assets dir");
                let dir_label = state
                    .last_export_dir
                    .as_ref()
                    .map(|d| d.display().to_string())
                    .unwrap_or_else(|| "(choose…)".into());
                if ui.button(dir_label).clicked() {
                    let mut dlg = rfd::FileDialog::new().set_title("Choose bestow assets directory");
                    if let Some(d) = &state.last_export_dir {
                        dlg = dlg.set_directory(d);
                    }
                    if let Some(dir) = dlg.pick_folder() {
                        state.last_export_dir = Some(dir);
                        state.save();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Asset prefix");
                ui.text_edit_singleline(&mut self.export.asset_prefix);
            });
            let can_export = state.last_export_dir.is_some();
            if ui.add_enabled(can_export, egui::Button::new("Export island")).clicked() {
                let out_dir = state.last_export_dir.clone().unwrap();
                let mut opts = wright_bestow::ExportOptions::new(&self.doc.name, out_dir);
                opts.asset_prefix = self.export.asset_prefix.trim_end_matches('/').to_string();
                match wright_bestow::export_island(&self.doc.field, &self.doc.masks, &opts) {
                    Ok(report) => {
                        self.status = format!(
                            "Exported {} files · h [{:.1}, {:.1}]",
                            report.files.len(),
                            report.height_min,
                            report.height_max
                        );
                        self.export.last_report = Some(report);
                        self.export.error = None;
                    }
                    Err(e) => self.export.error = Some(format!("{e:#}")),
                }
            }
            if let Some(err) = &self.export.error {
                ui.colored_label(Color32::LIGHT_RED, err);
            }
            if let Some(report) = &self.export.last_report {
                ui.add_space(6.0);
                egui::CollapsingHeader::new("Scene entity snippet").show(ui, |ui| {
                    ui.code(&report.entity_toml);
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(report.entity_toml.clone());
                    }
                });
                egui::CollapsingHeader::new(format!("{} files written", report.files.len())).show(
                    ui,
                    |ui| {
                        for f in &report.files {
                            ui.label(RichText::new(f.display().to_string()).small());
                        }
                    },
                );
            }
        });
    }

    fn status_bar(&mut self, root: &mut egui::Ui) {
        egui::Panel::bottom("status").show_inside(root, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(hit) = self.hover_hit {
                        ui.label(format!("{:.1}, {:.1}, {:.1}", hit.x, hit.y, hit.z));
                    }
                });
            });
        });
    }

    fn viewport(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(root, |ui| {
                let rect = ui.available_rect_before_wrap();
                let response = ui.allocate_rect(rect, Sense::click_and_drag());
                let ppp = ctx.pixels_per_point();
                let px_w = (rect.width() * ppp).round().max(1.0) as u32;
                let px_h = (rect.height() * ppp).round().max(1.0) as u32;
                let aspect = rect.width() / rect.height().max(1.0);

                // ── camera ────────────────────────────────────────────────
                let drag = response.drag_delta();
                let shift = ctx.input(|i| i.modifiers.shift);
                if response.dragged_by(PointerButton::Secondary) {
                    if shift {
                        self.camera.pan(drag.x, drag.y);
                    } else {
                        self.camera.orbit(drag.x, drag.y);
                    }
                } else if response.dragged_by(PointerButton::Middle) {
                    self.camera.pan(drag.x, drag.y);
                }
                if response.hovered() {
                    let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                    if scroll.abs() > 0.0 {
                        self.camera.dolly(scroll * 3.0);
                    }
                }
                if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(Key::F)) {
                    let (hmin, hmax) = self.doc.field.min_max();
                    self.camera
                        .frame(self.doc.field.world_size(), (hmin + hmax) * 0.5);
                }

                // ── picking ───────────────────────────────────────────────
                self.hover_hit = response.hover_pos().and_then(|pos| {
                    let u = (pos.x - rect.left()) / rect.width();
                    let v = (pos.y - rect.top()) / rect.height();
                    let (origin, dir) = self.camera.ray_through(u, v, aspect);
                    wright_field::raycast(&self.doc.field, origin, dir)
                });

                // ── sculpting ─────────────────────────────────────────────
                let pointer_down = response.dragged_by(PointerButton::Primary)
                    || (response.drag_started_by(PointerButton::Primary));
                if pointer_down {
                    if self.stroke.is_none() {
                        let anchor = self.hover_hit.map(|h| h.y).unwrap_or(0.0);
                        self.stroke = Some(ActiveStroke {
                            stroke: Stroke::new(self.brush, anchor),
                            pre_field: self.doc.field.clone(),
                            pre_masks: self.doc.masks.clone(),
                        });
                    }
                    if let (Some(active), Some(hit)) = (self.stroke.as_mut(), self.hover_hit) {
                        // strength feels better scaled up for sculpting clay
                        let dt = ctx.input(|i| i.stable_dt).min(0.05);
                        if let Some(region) = active.stroke.apply(
                            &mut self.doc.field,
                            &mut self.doc.masks,
                            hit.x,
                            hit.z,
                            dt,
                        ) {
                            self.scene
                                .upload_region(&self.doc.field, &self.doc.masks, region);
                        }
                    }
                } else if let Some(active) = self.stroke.take()
                    && let Some(region) = active.stroke.dirty
                {
                    self.doc
                        .commit_stroke(&active.pre_field, &active.pre_masks, region);
                }

                // ── render ────────────────────────────────────────────────
                let brush = match self.hover_hit {
                    Some(h) if !response.dragged_by(PointerButton::Secondary) => {
                        glam::Vec4::new(h.x, h.y, h.z, self.brush.radius)
                    }
                    _ => glam::Vec4::ZERO,
                };
                let brush_color = if self.brush.kind.is_material() {
                    [1.0, 0.85, 0.2, 0.9]
                } else {
                    [1.0, 1.0, 1.0, 0.8]
                };
                let params = SceneParams {
                    view_proj: self.camera.view_proj(aspect),
                    eye: self.camera.eye(),
                    brush,
                    brush_color,
                    time: self.time,
                };
                self.scene.render(px_w, px_h, &params);
                if let Some(id) = self.scene.texture_id {
                    ui.painter().image(
                        id,
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            });
    }

    fn new_island_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.new_dialog else {
            return;
        };
        let mut create = false;
        let mut cancel = false;
        egui::Window::new("New island")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut dialog.name);
                });
                ui.horizontal(|ui| {
                    ui.label("Resolution");
                    egui::ComboBox::from_id_salt("res")
                        .selected_text(format!("{0}×{0}", RESOLUTIONS[dialog.resolution_idx]))
                        .show_ui(ui, |ui| {
                            for (i, r) in RESOLUTIONS.iter().enumerate() {
                                ui.selectable_value(
                                    &mut dialog.resolution_idx,
                                    i,
                                    format!("{r}×{r}"),
                                );
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("World size (m)");
                    ui.add(egui::DragValue::new(&mut dialog.world_size).range(16.0..=8192.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Base height (m)");
                    ui.add(egui::DragValue::new(&mut dialog.base_height).range(-500.0..=500.0));
                });
                ui.label(
                    RichText::new(
                        "Start below sea level (negative) and raise\nland out of the water.",
                    )
                    .weak(),
                );
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() {
                        create = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if create {
            let d = self.new_dialog.take().unwrap();
            self.doc = IslandDoc::new(
                &d.name,
                RESOLUTIONS[d.resolution_idx],
                d.world_size,
                d.base_height,
            );
            self.camera = OrbitCamera::for_island(d.world_size);
            self.needs_full_upload = true;
            self.export.last_report = None;
        } else if cancel {
            self.new_dialog = None;
        }
    }

    // ── project io ────────────────────────────────────────────────────────

    fn save_project(&mut self, state: &mut AppState, force_dialog: bool) {
        let dir = if force_dialog || self.doc.project_dir.is_none() {
            let mut dlg = rfd::FileDialog::new()
                .set_title("Save island project")
                .set_file_name(format!("{}.wright", self.doc.name));
            if let Some(d) = state.last_project.as_ref().and_then(|p| p.parent()) {
                dlg = dlg.set_directory(d);
            }
            dlg.save_file()
        } else {
            self.doc.project_dir.clone()
        };
        let Some(dir) = dir else { return };
        match self.doc.save(&dir) {
            Ok(()) => {
                self.status = format!("Saved {}", dir.display());
                state.last_project = Some(dir);
                state.save();
            }
            Err(e) => self.status = format!("Save failed: {e:#}"),
        }
    }

    fn open_project(&mut self, state: &mut AppState) {
        let mut dlg = rfd::FileDialog::new().set_title("Open island project (.wright directory)");
        if let Some(d) = state.last_project.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(d);
        }
        let Some(dir) = dlg.pick_folder() else { return };
        match IslandDoc::load(&dir) {
            Ok(doc) => {
                self.camera = OrbitCamera::for_island(doc.field.world_size());
                self.doc = doc;
                self.needs_full_upload = true;
                self.export.last_report = None;
                self.status = format!("Opened {}", dir.display());
                state.last_project = Some(dir);
                state.save();
            }
            Err(e) => self.status = format!("Open failed: {e:#}"),
        }
    }
}
