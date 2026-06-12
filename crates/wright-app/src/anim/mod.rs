//! Animation mode: load a glTF rig, preview clips on a bone-line skeleton,
//! and author the metadata bestow needs — sockets on bones, event tags at
//! clip times, and sections that split clips into combo segments. Exports
//! the bestow animation TOML; the authored data also saves as a
//! `.wrightanim` project file (it IS the AnimMeta TOML).

use crate::render::camera::OrbitCamera;
use crate::render::scene::{LineVertex, SceneParams, SceneRenderer};
use crate::state::AppState;
use eframe::egui::{self, Color32, Key, PointerButton, RichText, Sense};
use eframe::egui_wgpu::RenderState;
use glam::Vec3;
use std::path::PathBuf;
use wright_anim::{AnimMeta, EventTag, Rig, Section, Socket};

pub struct AnimMode {
    rig: Option<Rig>,
    model_path: Option<PathBuf>,
    meta: AnimMeta,
    meta_path: Option<PathBuf>,
    selected_clip: usize,
    selected_bone: Option<usize>,
    time: f32,
    playing: bool,
    looping: bool,
    speed: f32,
    camera: OrbitCamera,
    scene: SceneRenderer,
    status: String,
    /// Pending section start time ("mark in"), if any.
    section_in: Option<f32>,
}

impl AnimMode {
    pub fn new(render_state: RenderState) -> Self {
        let mut camera = OrbitCamera::for_island(4.0);
        camera.focus = Vec3::new(0.0, 1.0, 0.0);
        Self {
            rig: None,
            model_path: None,
            meta: AnimMeta::default(),
            meta_path: None,
            selected_clip: 0,
            selected_bone: None,
            time: 0.0,
            playing: false,
            looping: true,
            speed: 1.0,
            camera,
            scene: SceneRenderer::new(render_state),
            status: "Open a model to begin (.glb / .gltf / .fbx)".into(),
            section_in: None,
        }
    }

    pub fn update(&mut self, root: &mut egui::Ui, _state: &mut AppState) {
        let ctx = root.ctx().clone();
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        if self.playing
            && let Some(clip) = self.current_clip()
        {
            let duration = clip.duration.max(1e-6);
            self.time += dt * self.speed;
            if self.looping {
                self.time = self.time.rem_euclid(duration);
            } else if self.time >= duration {
                self.time = duration;
                self.playing = false;
            }
        }

        self.bones_panel(root);
        self.meta_panel(root);
        self.timeline_panel(root);
        self.viewport(root);

        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }

    fn current_clip(&self) -> Option<&wright_anim::Clip> {
        self.rig.as_ref()?.clips.get(self.selected_clip)
    }

    fn clip_name(&self) -> String {
        self.current_clip()
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }

    // ── panels ────────────────────────────────────────────────────────────

    fn bones_panel(&mut self, root: &mut egui::Ui) {
        egui::Panel::left("anim_bones")
            .default_size(220.0)
            .show_inside(root, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Open model…").clicked() {
                        self.open_model();
                    }
                    if ui.button("Open meta…").clicked() {
                        self.open_meta();
                    }
                });
                let Some(rig) = &self.rig else {
                    ui.label(RichText::new("No model loaded.").weak());
                    return;
                };

                ui.separator();
                ui.heading(format!("Bones ({})", rig.bones.len()));
                // depth per bone for indentation
                let mut depth = vec![0usize; rig.bones.len()];
                for (i, b) in rig.bones.iter().enumerate() {
                    if let Some(p) = b.parent {
                        depth[i] = depth[p] + 1;
                    }
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, bone) in rig.bones.iter().enumerate() {
                        let label = format!("{}{}", "  ".repeat(depth[i]), bone.name);
                        if ui
                            .selectable_label(self.selected_bone == Some(i), label)
                            .clicked()
                        {
                            self.selected_bone = Some(i);
                        }
                    }
                });
            });
    }

    fn meta_panel(&mut self, root: &mut egui::Ui) {
        egui::Panel::right("anim_meta")
            .default_size(300.0)
            .show_inside(root, |ui| {
                ui.add_space(4.0);
                let Some(rig) = &self.rig else {
                    ui.heading("Animation metadata");
                    ui.label(
                        RichText::new(
                            "Sockets, event tags, and combo sections appear\nhere once a model is loaded.",
                        )
                        .weak(),
                    );
                    return;
                };
                let clip_name = self.clip_name();

                // ── clips ────────────────────────────────────────────────
                ui.heading("Clips");
                for (i, clip) in rig.clips.iter().enumerate() {
                    if ui
                        .selectable_label(
                            self.selected_clip == i,
                            format!("{} · {:.2}s", clip.name, clip.duration),
                        )
                        .clicked()
                    {
                        self.selected_clip = i;
                        self.time = 0.0;
                        self.section_in = None;
                    }
                }
                if rig.clips.is_empty() {
                    ui.label(RichText::new("Model has no animations.").weak());
                }

                // ── sockets ──────────────────────────────────────────────
                ui.add_space(8.0);
                ui.separator();
                ui.heading("Sockets");
                let bone_name = self
                    .selected_bone
                    .and_then(|i| rig.bones.get(i))
                    .map(|b| b.name.clone());
                let add_label = match &bone_name {
                    Some(b) => format!("+ socket on `{b}`"),
                    None => "+ socket (select a bone)".into(),
                };
                if ui
                    .add_enabled(bone_name.is_some(), egui::Button::new(add_label))
                    .clicked()
                    && let Some(bone) = bone_name
                {
                    self.meta.sockets.push(Socket {
                        name: format!("socket_{}", self.meta.sockets.len() + 1),
                        bone,
                        offset: [0.0; 3],
                        rotation: [0.0, 0.0, 0.0, 1.0],
                    });
                }
                let mut remove = None;
                for (i, socket) in self.meta.sockets.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut socket.name);
                        ui.label(RichText::new(format!("@ {}", socket.bone)).weak());
                        if ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("offset");
                        for c in 0..3 {
                            ui.add(
                                egui::DragValue::new(&mut socket.offset[c])
                                    .speed(0.005)
                                    .max_decimals(3),
                            );
                        }
                    });
                }
                if let Some(i) = remove {
                    self.meta.sockets.remove(i);
                }

                // ── events for the selected clip ─────────────────────────
                ui.add_space(8.0);
                ui.separator();
                ui.heading(format!("Events · {clip_name}"));
                if ui.button("+ event at playhead").clicked() {
                    self.meta.events.push(EventTag {
                        clip: clip_name.clone(),
                        name: "event".into(),
                        time: (self.time * 1000.0).round() / 1000.0,
                        payload: vec![],
                    });
                }
                let mut remove = None;
                for (i, ev) in self.meta.events.iter_mut().enumerate() {
                    if ev.clip != clip_name {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut ev.name);
                        ui.add(
                            egui::DragValue::new(&mut ev.time)
                                .speed(0.01)
                                .range(0.0..=1e4)
                                .suffix(" s"),
                        );
                        if ui.small_button("⏵").on_hover_text("jump here").clicked() {
                            self.time = ev.time;
                            self.playing = false;
                        }
                        if ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    self.meta.events.remove(i);
                }

                // ── sections (combo splitting) ───────────────────────────
                ui.add_space(8.0);
                ui.separator();
                ui.heading(format!("Sections · {clip_name}"));
                ui.horizontal(|ui| {
                    match self.section_in {
                        None => {
                            if ui.button("Mark in").clicked() {
                                self.section_in = Some(self.time);
                            }
                        }
                        Some(start) => {
                            if ui.button(format!("Mark out ({start:.2}s →)")).clicked() {
                                let (a, b) = if self.time >= start {
                                    (start, self.time)
                                } else {
                                    (self.time, start)
                                };
                                self.meta.sections.push(Section {
                                    clip: clip_name.clone(),
                                    name: format!(
                                        "part_{}",
                                        self.meta.sections.iter().filter(|s| s.clip == clip_name).count() + 1
                                    ),
                                    start: (a * 1000.0).round() / 1000.0,
                                    end: (b * 1000.0).round() / 1000.0,
                                    can_end: false,
                                });
                                self.section_in = None;
                            }
                            if ui.small_button("cancel").clicked() {
                                self.section_in = None;
                            }
                        }
                    }
                });
                let mut remove = None;
                for (i, sec) in self.meta.sections.iter_mut().enumerate() {
                    if sec.clip != clip_name {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut sec.name);
                        ui.add(egui::DragValue::new(&mut sec.start).speed(0.01).suffix(" s"));
                        ui.label("→");
                        ui.add(egui::DragValue::new(&mut sec.end).speed(0.01).suffix(" s"));
                        ui.checkbox(&mut sec.can_end, "can end")
                            .on_hover_text("player may exit the combo after this section");
                        if ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    self.meta.sections.remove(i);
                }

                // ── save / export ────────────────────────────────────────
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save meta").clicked() {
                        self.save_meta();
                    }
                    if ui.button("Export bestow TOML…").clicked() {
                        self.export_bestow();
                    }
                });
            });
    }

    fn timeline_panel(&mut self, root: &mut egui::Ui) {
        egui::Panel::bottom("anim_timeline").show_inside(root, |ui| {
            let Some(clip) = self.current_clip() else {
                ui.label(&self.status);
                return;
            };
            let duration = clip.duration.max(1e-6);
            let clip_name = clip.name.clone();
            ui.horizontal(|ui| {
                if ui.button(if self.playing { "⏸" } else { "▶" }).clicked() {
                    self.playing = !self.playing;
                    if self.playing && self.time >= duration {
                        self.time = 0.0;
                    }
                }
                ui.toggle_value(&mut self.looping, "loop");
                ui.add(
                    egui::Slider::new(&mut self.speed, 0.1..=3.0)
                        .text("speed")
                        .logarithmic(true),
                );
                ui.spacing_mut().slider_width = ui.available_width() - 120.0;
                let label = format!("{:.2}s", self.time);
                ui.add(egui::Slider::new(&mut self.time, 0.0..=duration).text(label));
            });

            // marker strip: events (gold) and sections (teal spans)
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), Sense::click());
            let painter = ui.painter();
            painter.rect_filled(rect, 2.0, Color32::from_gray(28));
            let to_x = |t: f32| rect.left() + (t / duration).clamp(0.0, 1.0) * rect.width();
            for sec in self.meta.sections.iter().filter(|s| s.clip == clip_name) {
                let span = egui::Rect::from_min_max(
                    egui::pos2(to_x(sec.start), rect.top() + 3.0),
                    egui::pos2(to_x(sec.end), rect.bottom() - 3.0),
                );
                let color = if sec.can_end {
                    Color32::from_rgb(40, 140, 130)
                } else {
                    Color32::from_rgb(50, 95, 120)
                };
                painter.rect_filled(span, 2.0, color);
            }
            for ev in self.meta.events.iter().filter(|e| e.clip == clip_name) {
                let x = to_x(ev.time);
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    egui::Stroke::new(2.0, Color32::from_rgb(230, 190, 60)),
                );
            }
            if let Some(start) = self.section_in {
                let x = to_x(start);
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    egui::Stroke::new(2.0, Color32::from_rgb(80, 220, 200)),
                );
            }
            let x = to_x(self.time);
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(2.0, Color32::WHITE),
            );
            if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                self.time = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * duration;
                self.playing = false;
            }
        });
    }

    // ── viewport ──────────────────────────────────────────────────────────

    fn viewport(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(root, |ui| {
                let rect = ui.available_rect_before_wrap();
                let response = ui.allocate_rect(rect, Sense::click_and_drag());
                let ppp = ctx.pixels_per_point();
                let px_w = (rect.width() * ppp).round().max(1.0) as u32;
                let px_h = (rect.height() * ppp).round().max(1.0) as u32;
                let aspect = rect.width() / rect.height().max(1.0);

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
                if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(Key::Space)) {
                    self.playing = !self.playing;
                }

                // skeleton → overlay lines (+ bone click-picking data)
                let mut lines = grid_lines();
                let mut joints_screen: Vec<(usize, egui::Pos2)> = Vec::new();
                if let Some(rig) = &self.rig {
                    let locals = match self.current_clip() {
                        Some(clip) => clip.sample_pose(rig, self.time),
                        None => rig.rest_pose(),
                    };
                    let world = rig.world_matrices(&locals);
                    let view_proj = self.camera.view_proj(aspect);
                    for (i, bone) in rig.bones.iter().enumerate() {
                        let pos = world[i].to_scale_rotation_translation().2;
                        let selected = self.selected_bone == Some(i);
                        if let Some(p) = bone.parent {
                            let parent_pos = world[p].to_scale_rotation_translation().2;
                            let color = if selected {
                                [1.0, 0.8, 0.2, 1.0]
                            } else {
                                [0.85, 0.9, 1.0, 0.9]
                            };
                            lines.push(LineVertex::new(parent_pos, color));
                            lines.push(LineVertex::new(pos, color));
                        }
                        // joint cross
                        let s = if selected { 0.035 } else { 0.02 };
                        let c = if selected {
                            [1.0, 0.8, 0.2, 1.0]
                        } else {
                            [0.4, 0.95, 0.6, 0.9]
                        };
                        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                            lines.push(LineVertex::new(pos - axis * s, c));
                            lines.push(LineVertex::new(pos + axis * s, c));
                        }
                        // screen position for click picking
                        let clip_pos = view_proj * pos.extend(1.0);
                        if clip_pos.w > 0.0 {
                            let ndc = clip_pos / clip_pos.w;
                            joints_screen.push((
                                i,
                                egui::pos2(
                                    rect.left() + (ndc.x * 0.5 + 0.5) * rect.width(),
                                    rect.top() + (0.5 - ndc.y * 0.5) * rect.height(),
                                ),
                            ));
                        }
                    }
                    // sockets as magenta tripods
                    for socket in &self.meta.sockets {
                        if let Some(&bi) = rig.by_name.get(&socket.bone) {
                            let m = world[bi]
                                * glam::Mat4::from_rotation_translation(
                                    glam::Quat::from_array(socket.rotation),
                                    Vec3::from_array(socket.offset),
                                );
                            let pos = m.to_scale_rotation_translation().2;
                            let c = [1.0, 0.35, 0.9, 1.0];
                            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                                let dir = m.transform_vector3(axis * 0.05);
                                lines.push(LineVertex::new(pos, c));
                                lines.push(LineVertex::new(pos + dir, c));
                            }
                        }
                    }
                }
                self.scene.set_lines(&lines);

                // click to select the nearest joint
                if response.clicked()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let best = joints_screen
                        .iter()
                        .map(|(i, p)| (*i, p.distance(pos)))
                        .min_by(|a, b| a.1.total_cmp(&b.1));
                    if let Some((i, d)) = best
                        && d < 14.0
                    {
                        self.selected_bone = Some(i);
                    }
                }

                let params = SceneParams {
                    view_proj: self.camera.view_proj(aspect),
                    eye: self.camera.eye(),
                    brush: glam::Vec4::ZERO,
                    brush_color: [0.0; 4],
                    time: self.time,
                    water: false,
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
                // status overlay
                ui.painter().text(
                    rect.left_bottom() + egui::vec2(8.0, -8.0),
                    egui::Align2::LEFT_BOTTOM,
                    &self.status,
                    egui::FontId::proportional(12.0),
                    Color32::from_white_alpha(180),
                );
            });
    }

    // ── io ────────────────────────────────────────────────────────────────

    fn open_model(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open model (glTF or FBX)")
            .add_filter("models", &["glb", "gltf", "fbx"])
            .pick_file()
        else {
            return;
        };
        match wright_anim::load_model(&path) {
            Ok(rig) => {
                self.status = format!(
                    "{}: {} bones, {} clips",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    rig.bones.len(),
                    rig.clips.len()
                );
                self.meta.model = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into();
                self.rig = Some(rig);
                self.model_path = Some(path);
                self.selected_clip = 0;
                self.selected_bone = None;
                self.time = 0.0;
                self.section_in = None;
            }
            Err(e) => self.status = format!("Load failed: {e:#}"),
        }
    }

    fn save_meta(&mut self) {
        let path = self.meta_path.clone().or_else(|| {
            let stem = self
                .model_path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "rig".into());
            rfd::FileDialog::new()
                .set_title("Save animation metadata")
                .set_file_name(format!("{stem}.wrightanim"))
                .save_file()
        });
        let Some(path) = path else { return };
        match toml::to_string_pretty(&self.meta)
            .map_err(anyhow::Error::from)
            .and_then(|s| std::fs::write(&path, s).map_err(Into::into))
        {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                self.meta_path = Some(path);
            }
            Err(e) => self.status = format!("Save failed: {e:#}"),
        }
    }

    fn open_meta(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open animation metadata")
            .add_filter("wright anim meta", &["wrightanim", "toml"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|s| toml::from_str::<AnimMeta>(&s).map_err(Into::into))
        {
            Ok(meta) => {
                self.meta = meta;
                self.meta_path = Some(path);
                self.status = format!(
                    "Loaded meta: {} sockets, {} events, {} sections",
                    self.meta.sockets.len(),
                    self.meta.events.len(),
                    self.meta.sections.len()
                );
            }
            Err(e) => self.status = format!("Open failed: {e:#}"),
        }
    }

    fn export_bestow(&mut self) {
        let stem = self
            .model_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "rig".into());
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export bestow animation TOML")
            .set_file_name(format!("{stem}.anim.toml"))
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, self.meta.to_bestow_toml()) {
            Ok(()) => self.status = format!("Exported {}", path.display()),
            Err(e) => self.status = format!("Export failed: {e:#}"),
        }
    }
}

/// 10×10 m reference grid on the ground plane, meter spacing.
fn grid_lines() -> Vec<LineVertex> {
    let mut lines = Vec::with_capacity(44 * 2);
    let half = 5;
    for i in -half..=half {
        let (a, c) = (i as f32, half as f32);
        let color = if i == 0 {
            [0.6, 0.6, 0.65, 0.8]
        } else {
            [0.35, 0.38, 0.42, 0.6]
        };
        lines.push(LineVertex::new(Vec3::new(a, 0.0, -c), color));
        lines.push(LineVertex::new(Vec3::new(a, 0.0, c), color));
        lines.push(LineVertex::new(Vec3::new(-c, 0.0, a), color));
        lines.push(LineVertex::new(Vec3::new(c, 0.0, a), color));
    }
    lines
}
