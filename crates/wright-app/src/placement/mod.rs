//! Placement mode: load an exported island as the ground, click to place
//! entity templates (snapped to the terrain), arrange and tag them, and
//! export bestow `[[entities]]` scene TOML. Ground rendering, picking, and
//! line markers all reuse the island/animation viewport machinery.

use crate::render::camera::OrbitCamera;
use crate::render::scene::{LineVertex, SceneParams, SceneRenderer};
use crate::state::AppState;
use eframe::egui::{self, Color32, PointerButton, RichText, Sense};
use eframe::egui_wgpu::RenderState;
use glam::Vec3;
use std::path::PathBuf;
use wright_bestow::{PlacedEntity, SceneDoc};
use wright_field::Heightfield;

pub struct PlacementMode {
    doc: SceneDoc,
    doc_path: Option<PathBuf>,
    ground: Option<Heightfield>,
    camera: OrbitCamera,
    scene: SceneRenderer,
    selected: Option<usize>,
    /// Template name applied to the next placement click.
    template: String,
    recent_templates: Vec<String>,
    status: String,
    counter: usize,
}

impl PlacementMode {
    pub fn new(render_state: RenderState) -> Self {
        Self {
            doc: SceneDoc {
                name: "placement".into(),
                ..Default::default()
            },
            doc_path: None,
            ground: None,
            camera: OrbitCamera::for_island(256.0),
            scene: SceneRenderer::new(render_state),
            selected: None,
            template: String::new(),
            recent_templates: Vec::new(),
            status: "Load a ground island, type a template name, click terrain to place.".into(),
            counter: 0,
        }
    }

    pub fn update(&mut self, root: &mut egui::Ui, state: &mut AppState) {
        self.entities_panel(root, state);
        self.viewport(root);
        root.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }

    // ── side panel ────────────────────────────────────────────────────────

    fn entities_panel(&mut self, root: &mut egui::Ui, state: &mut AppState) {
        egui::Panel::right("placement_panel")
            .default_size(300.0)
            .show_inside(root, |ui| {
                ui.add_space(4.0);
                ui.heading("Scene");
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut self.doc.name);
                });
                ui.horizontal(|ui| {
                    if ui.button("Ground…").clicked() {
                        self.load_ground(state);
                    }
                    if ui.button("Open…").clicked() {
                        self.open_project();
                    }
                    if ui.button("Save").clicked() {
                        self.save_project(false);
                    }
                    if ui.button("Save as…").clicked() {
                        self.save_project(true);
                    }
                });
                if let Some(g) = &self.doc.ground {
                    ui.label(RichText::new(g.as_str()).weak().small());
                }

                ui.add_space(8.0);
                ui.separator();
                ui.heading("Place");
                ui.horizontal(|ui| {
                    ui.label("Template");
                    ui.text_edit_singleline(&mut self.template);
                });
                if !self.recent_templates.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        for t in self.recent_templates.clone() {
                            if ui.small_button(&t).clicked() {
                                self.template = t;
                            }
                        }
                    });
                }
                ui.label(
                    RichText::new("Click terrain to place · click a marker to select")
                        .weak()
                        .small(),
                );

                ui.add_space(8.0);
                ui.separator();
                ui.heading(format!("Entities ({})", self.doc.entities.len()));
                let mut remove = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for i in 0..self.doc.entities.len() {
                        let is_sel = self.selected == Some(i);
                        let label = {
                            let e = &self.doc.entities[i];
                            format!(
                                "{} {}",
                                if e.template.is_empty() {
                                    "·"
                                } else {
                                    &e.template
                                },
                                e.name
                            )
                        };
                        if ui.selectable_label(is_sel, label).clicked() {
                            self.selected = Some(i);
                        }
                        if is_sel {
                            let e = &mut self.doc.entities[i];
                            ui.horizontal(|ui| {
                                ui.label("name");
                                ui.text_edit_singleline(&mut e.name);
                            });
                            ui.horizontal(|ui| {
                                ui.label("pos");
                                for c in 0..3 {
                                    ui.add(
                                        egui::DragValue::new(&mut e.position[c])
                                            .speed(0.1)
                                            .max_decimals(2),
                                    );
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("yaw");
                                ui.add(egui::DragValue::new(&mut e.yaw_deg).speed(1.0).suffix("°"));
                                let mut tags = e.tags.join(", ");
                                ui.label("tags");
                                if ui.text_edit_singleline(&mut tags).changed() {
                                    e.tags = tags
                                        .split(',')
                                        .map(|t| t.trim().to_string())
                                        .filter(|t| !t.is_empty())
                                        .collect();
                                }
                            });
                            ui.horizontal(|ui| {
                                if ui.small_button("snap to ground").clicked()
                                    && let Some(g) = &self.ground
                                    && let Some(h) = g.height_at(e.position[0], e.position[2])
                                {
                                    e.position[1] = h;
                                }
                                if ui.small_button("✕ delete").clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                    }
                });
                if let Some(i) = remove {
                    self.doc.entities.remove(i);
                    self.selected = None;
                }

                ui.add_space(8.0);
                ui.separator();
                if ui.button("Export scene TOML…").clicked() {
                    self.export_scene();
                }
                ui.label(RichText::new(&self.status).weak());
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

                // entity markers + screen positions for click selection
                let view_proj = self.camera.view_proj(aspect);
                let mut lines = Vec::new();
                let mut markers_screen: Vec<(usize, egui::Pos2)> = Vec::new();
                for (i, e) in self.doc.entities.iter().enumerate() {
                    let p = Vec3::from_array(e.position);
                    let selected = self.selected == Some(i);
                    let c = if selected {
                        [1.0, 0.8, 0.2, 1.0]
                    } else {
                        [0.3, 0.8, 1.0, 0.95]
                    };
                    // pole + head diamond + yaw tick
                    let top = p + Vec3::Y * 2.0;
                    lines.push(LineVertex::new(p, c));
                    lines.push(LineVertex::new(top, c));
                    let s = 0.35;
                    for (a, b) in [
                        (Vec3::X * s, Vec3::Z * s),
                        (Vec3::Z * s, -Vec3::X * s),
                        (-Vec3::X * s, -Vec3::Z * s),
                        (-Vec3::Z * s, Vec3::X * s),
                    ] {
                        lines.push(LineVertex::new(top + a, c));
                        lines.push(LineVertex::new(top + b, c));
                    }
                    let yaw = e.yaw_deg.to_radians();
                    let dir = Vec3::new(yaw.sin(), 0.0, yaw.cos());
                    lines.push(LineVertex::new(top, c));
                    lines.push(LineVertex::new(top + dir * 1.2, c));

                    let clip = view_proj * top.extend(1.0);
                    if clip.w > 0.0 {
                        let ndc = clip / clip.w;
                        markers_screen.push((
                            i,
                            egui::pos2(
                                rect.left() + (ndc.x * 0.5 + 0.5) * rect.width(),
                                rect.top() + (0.5 - ndc.y * 0.5) * rect.height(),
                            ),
                        ));
                    }
                }
                self.scene.set_lines(&lines);

                // click: select nearby marker, else place at terrain hit
                if response.clicked()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let nearest = markers_screen
                        .iter()
                        .map(|(i, p)| (*i, p.distance(pos)))
                        .min_by(|a, b| a.1.total_cmp(&b.1));
                    if let Some((i, d)) = nearest
                        && d < 14.0
                    {
                        self.selected = Some(i);
                    } else if let Some(ground) = &self.ground {
                        let u = (pos.x - rect.left()) / rect.width();
                        let v = (pos.y - rect.top()) / rect.height();
                        let (origin, dir) = self.camera.ray_through(u, v, aspect);
                        if let Some(hit) = wright_field::raycast(ground, origin, dir) {
                            self.place(hit);
                        }
                    }
                }

                let params = SceneParams {
                    view_proj,
                    eye: self.camera.eye(),
                    brush: glam::Vec4::ZERO,
                    brush_color: [0.0; 4],
                    time: 0.0,
                    water: true,
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

    fn place(&mut self, hit: Vec3) {
        let template = self.template.trim().to_string();
        let stem = if template.is_empty() {
            "entity"
        } else {
            template.as_str()
        };
        // first free suffix — deletions and project reloads must never
        // produce a duplicate name (bestow scene loads fail on name_taken)
        let name = (self.counter + 1..)
            .map(|i| format!("{stem}_{i}"))
            .find(|n| !self.doc.entities.iter().any(|e| &e.name == n))
            .unwrap();
        self.counter += 1;
        if !template.is_empty() && !self.recent_templates.contains(&template) {
            self.recent_templates.insert(0, template.clone());
            self.recent_templates.truncate(8);
        }
        self.doc.entities.push(PlacedEntity {
            name,
            template,
            tags: Vec::new(),
            position: hit.to_array(),
            yaw_deg: 0.0,
        });
        self.selected = Some(self.doc.entities.len() - 1);
    }

    // ── io ────────────────────────────────────────────────────────────────

    fn load_ground(&mut self, state: &mut AppState) {
        let mut dlg = rfd::FileDialog::new()
            .set_title("Load ground island (pick its .hgt.toml)")
            .add_filter("island metadata", &["toml"]);
        if let Some(d) = &state.last_export_dir {
            dlg = dlg.set_directory(d);
        }
        let Some(path) = dlg.pick_file() else { return };
        self.set_ground(&path);
    }

    fn set_ground(&mut self, path: &std::path::Path) {
        match wright_bestow::import_island(path) {
            Ok((field, masks, name)) => {
                self.camera = OrbitCamera::for_island(field.world_size());
                self.scene.upload_all(&field, &masks);
                self.ground = Some(field);
                self.doc.ground = Some(path.display().to_string());
                self.status = format!("Ground: {name}");
            }
            Err(e) => self.status = format!("Ground load failed: {e:#}"),
        }
    }

    fn save_project(&mut self, force_dialog: bool) {
        let path = if force_dialog || self.doc_path.is_none() {
            rfd::FileDialog::new()
                .set_title("Save placement project")
                .set_file_name(format!("{}.wrightscene", self.doc.name))
                .save_file()
        } else {
            self.doc_path.clone()
        };
        let Some(path) = path else { return };
        match self.doc.save(&path) {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                self.doc_path = Some(path);
            }
            Err(e) => self.status = format!("Save failed: {e:#}"),
        }
    }

    fn open_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open placement project")
            .add_filter("wright scene", &["wrightscene", "toml"])
            .pick_file()
        else {
            return;
        };
        match SceneDoc::load(&path) {
            Ok(doc) => {
                self.counter = doc.entities.len();
                if let Some(g) = doc.ground.clone() {
                    self.set_ground(std::path::Path::new(&g));
                }
                self.doc = doc;
                self.doc_path = Some(path);
                self.selected = None;
            }
            Err(e) => self.status = format!("Open failed: {e:#}"),
        }
    }

    fn export_scene(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export bestow scene TOML")
            .set_file_name(format!("{}.entities.toml", self.doc.name))
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, self.doc.to_scene_toml()) {
            Ok(()) => self.status = format!("Exported {}", path.display()),
            Err(e) => self.status = format!("Export failed: {e:#}"),
        }
    }
}
