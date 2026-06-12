//! Dungeon mode: paint Zelda-scale dungeons on a cell grid — floors, door
//! edges, entities — preview the generated shell live in 3D (backface
//! culling gives a natural dollhouse view from above), and export the
//! whole dungeon as one self-contained bestow asset folder.

use crate::render::camera::OrbitCamera;
use crate::render::scene::{LineVertex, SceneParams, SceneRenderer};
use crate::state::AppState;
use eframe::egui::{self, Color32, PointerButton, RichText, Sense};
use eframe::egui_wgpu::RenderState;
use glam::Vec3;
use std::path::PathBuf;
use wright_dungeon::{Cell, Door, DoorKind, DungeonDoc, DungeonEntity, export, meshgen};
use wright_field::Vertex;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    PaintFloor,
    EraseFloor,
    PlaceDoor,
    PlaceEntity,
    Select,
}

impl Tool {
    const ALL: [Tool; 5] = [
        Tool::PaintFloor,
        Tool::EraseFloor,
        Tool::PlaceDoor,
        Tool::PlaceEntity,
        Tool::Select,
    ];

    fn label(self) -> &'static str {
        match self {
            Tool::PaintFloor => "Paint floor",
            Tool::EraseFloor => "Erase",
            Tool::PlaceDoor => "Place door",
            Tool::PlaceEntity => "Place entity",
            Tool::Select => "Select",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    Door(usize),
    Entity(usize),
}

pub struct DungeonMode {
    doc: DungeonDoc,
    doc_path: Option<PathBuf>,
    camera: OrbitCamera,
    scene: SceneRenderer,
    tool: Tool,
    storey: usize,
    door_kind_idx: usize,
    door_key: String,
    template: String,
    selection: Option<Selection>,
    show_ceilings: bool,
    needs_remesh: bool,
    dirty_since_save: bool,
    status: String,
    last_export: Option<export::ExportReport>,
    export_error: Option<String>,
    new_dialog: Option<NewDungeonDialog>,
    /// Cell hovered this frame on the active storey, plus the nearest edge
    /// (for the door tool).
    hover: Option<Hover>,
}

struct NewDungeonDialog {
    name: String,
    width: usize,
    depth: usize,
    cell_size: f32,
    wall_height: f32,
}

#[derive(Clone, Copy)]
struct Hover {
    cell: (usize, usize),
    /// Adjacent cell across the nearest edge (may be out of bounds).
    across: (i64, i64),
    world: Vec3,
}

const DOOR_KINDS: [&str; 3] = ["open", "locked", "boss"];

impl DungeonMode {
    pub fn new(render_state: RenderState) -> Self {
        let doc = DungeonDoc::new("dungeon", 32, 32);
        let mut camera = OrbitCamera::for_island(doc.floors[0].width as f32 * doc.cell_size);
        camera.pitch = -1.1; // start near top-down: the floor-plan view
        Self {
            doc,
            doc_path: None,
            camera,
            scene: SceneRenderer::new(render_state),
            tool: Tool::PaintFloor,
            storey: 0,
            door_kind_idx: 1,
            door_key: "small_key".into(),
            template: String::new(),
            selection: None,
            show_ceilings: false,
            needs_remesh: true,
            dirty_since_save: false,
            status:
                "Paint floor cells, place doors on edges between floor cells, export the folder."
                    .into(),
            last_export: None,
            export_error: None,
            new_dialog: None,
            hover: None,
        }
    }

    pub fn update(&mut self, root: &mut egui::Ui, state: &mut AppState) {
        if self.needs_remesh {
            self.remesh();
            self.needs_remesh = false;
        }
        self.tools_panel(root);
        self.dungeon_panel(root, state);
        self.viewport(root);
        self.new_dungeon_dialog(&root.ctx().clone());
        root.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }

    /// Shell mesh → tinted vertices through the terrain preview pipeline.
    fn remesh(&mut self) {
        let mesh = meshgen::generate(&self.doc);
        let tint = |prim: &meshgen::Primitive, color: [f32; 3]| {
            let vertices: Vec<Vertex> = prim
                .positions
                .iter()
                .zip(&prim.normals)
                .map(|(p, n)| Vertex {
                    position: *p,
                    normal: *n,
                    // autoshader off, rockness 0 → albedo = grass * tint;
                    // tint carries the actual surface color
                    material: [0.0, 0.0, 0.0, 0.0],
                    tint: [color[0] * 3.0, color[1] * 3.0, color[2] * 3.0],
                })
                .collect();
            (vertices, prim.indices.clone())
        };
        let mut slices = vec![
            tint(&mesh.floor, [0.50, 0.44, 0.36]),
            tint(&mesh.wall, [0.55, 0.53, 0.50]),
        ];
        if self.show_ceilings {
            slices.push(tint(&mesh.ceiling, [0.30, 0.29, 0.28]));
        }
        self.scene.set_custom_mesh(slices);
    }

    fn touch(&mut self) {
        self.needs_remesh = true;
        self.dirty_since_save = true;
    }

    // ── panels ────────────────────────────────────────────────────────────

    fn tools_panel(&mut self, root: &mut egui::Ui) {
        egui::Panel::left("dungeon_tools")
            .default_size(200.0)
            .show_inside(root, |ui| {
                ui.add_space(4.0);
                ui.heading("Tools");
                for (i, tool) in Tool::ALL.into_iter().enumerate() {
                    let label = format!("{}  {}", i + 1, tool.label());
                    if ui.selectable_label(self.tool == tool, label).clicked() {
                        self.tool = tool;
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                ui.heading("Door");
                ui.horizontal(|ui| {
                    for (i, kind) in DOOR_KINDS.iter().enumerate() {
                        if ui
                            .selectable_label(self.door_kind_idx == i, *kind)
                            .clicked()
                        {
                            self.door_kind_idx = i;
                        }
                    }
                });
                if self.door_kind_idx == 1 {
                    ui.horizontal(|ui| {
                        ui.label("key");
                        ui.text_edit_singleline(&mut self.door_key);
                    });
                }

                ui.add_space(8.0);
                ui.separator();
                ui.heading("Entity");
                ui.horizontal(|ui| {
                    ui.label("template");
                    ui.text_edit_singleline(&mut self.template);
                });
                ui.label(
                    RichText::new("blank template = marker entity\n(tags only, e.g. player_spawn)")
                        .weak()
                        .small(),
                );

                ui.add_space(8.0);
                ui.separator();
                ui.heading("Storey");
                ui.horizontal(|ui| {
                    for i in 0..self.doc.floors.len() {
                        if ui
                            .selectable_label(self.storey == i, format!("{i}"))
                            .clicked()
                        {
                            self.storey = i;
                        }
                    }
                    if ui.button("+").clicked() {
                        let (w, d) = (self.doc.floors[0].width, self.doc.floors[0].depth);
                        self.doc.floors.push(wright_dungeon::Floor::new(w, d));
                        self.storey = self.doc.floors.len() - 1;
                        self.touch();
                    }
                });
                if ui
                    .checkbox(&mut self.show_ceilings, "show ceilings")
                    .changed()
                {
                    self.needs_remesh = true;
                }
            });
    }

    fn dungeon_panel(&mut self, root: &mut egui::Ui, state: &mut AppState) {
        egui::Panel::right("dungeon_panel")
            .default_size(310.0)
            .show_inside(root, |ui| {
                ui.add_space(4.0);
                ui.heading("Dungeon");
                ui.horizontal(|ui| {
                    ui.label("Name");
                    if ui.text_edit_singleline(&mut self.doc.name).changed() {
                        self.dirty_since_save = true;
                    }
                });
                let cells: usize = self
                    .doc
                    .floors
                    .iter()
                    .map(wright_dungeon::Floor::floor_count)
                    .sum();
                ui.label(format!(
                    "{} storeys · {} floor cells · {} doors · {} entities",
                    self.doc.floors.len(),
                    cells,
                    self.doc.doors.len(),
                    self.doc.entities.len()
                ));
                ui.horizontal(|ui| {
                    if ui.button("New…").clicked() {
                        self.new_dialog = Some(NewDungeonDialog {
                            name: "dungeon".into(),
                            width: 32,
                            depth: 32,
                            cell_size: 2.0,
                            wall_height: 4.0,
                        });
                    }
                    if ui.button("Open…").clicked() {
                        self.open_project();
                    }
                    let label = if self.dirty_since_save { "Save*" } else { "Save" };
                    if ui.button(label).clicked() {
                        self.save_project(false);
                    }
                    if ui.button("Save as…").clicked() {
                        self.save_project(true);
                    }
                });

                // ── selection ────────────────────────────────────────────
                ui.add_space(8.0);
                ui.separator();
                match self.selection {
                    Some(Selection::Door(i)) if i < self.doc.doors.len() => {
                        self.door_editor(ui, i);
                    }
                    Some(Selection::Entity(i)) if i < self.doc.entities.len() => {
                        self.entity_editor(ui, i);
                    }
                    _ => {
                        ui.heading("Doors & entities");
                        ui.label(
                            RichText::new("Select with the Select tool, or from the lists:")
                                .weak()
                                .small(),
                        );
                        let mut select = None;
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            for (i, d) in self.doc.doors.iter().enumerate() {
                                if ui
                                    .selectable_label(false, format!("🚪 {} ({})", d.name, d.kind.label()))
                                    .clicked()
                                {
                                    select = Some(Selection::Door(i));
                                }
                            }
                            for (i, e) in self.doc.entities.iter().enumerate() {
                                let label = if e.template.is_empty() {
                                    format!("· {}", e.name)
                                } else {
                                    format!("{} {}", e.template, e.name)
                                };
                                if ui.selectable_label(false, label).clicked() {
                                    select = Some(Selection::Entity(i));
                                }
                            }
                        });
                        if select.is_some() {
                            self.selection = select;
                        }
                    }
                }

                // ── validation ───────────────────────────────────────────
                ui.add_space(8.0);
                ui.separator();
                ui.heading("Validation");
                let issues = self.doc.validate();
                if issues.is_empty() {
                    ui.colored_label(Color32::from_rgb(110, 200, 120), "✓ ready to export");
                }
                for issue in &issues {
                    let color = if issue.error {
                        Color32::LIGHT_RED
                    } else {
                        Color32::from_rgb(230, 190, 60)
                    };
                    ui.colored_label(color, &issue.message);
                }

                // ── export ───────────────────────────────────────────────
                ui.add_space(8.0);
                ui.separator();
                ui.heading("Export to bestow");
                ui.label(
                    RichText::new("One folder: scene + shell glb + door template\n+ UUID sidecars, under <game>/assets/dungeons/")
                        .weak()
                        .small(),
                );
                ui.horizontal(|ui| {
                    ui.label("dungeons dir");
                    let label = state
                        .last_dungeon_dir
                        .as_ref()
                        .map(|d| d.display().to_string())
                        .unwrap_or_else(|| "(choose…)".into());
                    if ui.button(label).clicked() {
                        let mut dlg = rfd::FileDialog::new()
                            .set_title("Choose the game's assets/dungeons directory");
                        if let Some(d) = &state.last_dungeon_dir {
                            dlg = dlg.set_directory(d);
                        }
                        if let Some(dir) = dlg.pick_folder() {
                            state.last_dungeon_dir = Some(dir);
                            state.save();
                        }
                    }
                });
                let can = state.last_dungeon_dir.is_some()
                    && !issues.iter().any(|i| i.error);
                if ui.add_enabled(can, egui::Button::new("Export dungeon")).clicked() {
                    let dir = state.last_dungeon_dir.clone().unwrap();
                    match export::export_dungeon(&self.doc, &dir, "assets/dungeons") {
                        Ok(report) => {
                            self.status = format!(
                                "Exported {} files · {} tris · play: scene.load(\"{}\")",
                                report.files.len(),
                                report.triangle_count,
                                report.scene_rel
                            );
                            self.last_export = Some(report);
                            self.export_error = None;
                        }
                        Err(e) => self.export_error = Some(format!("{e:#}")),
                    }
                }
                if let Some(err) = &self.export_error {
                    ui.colored_label(Color32::LIGHT_RED, err);
                }
                if let Some(report) = &self.last_export {
                    let load_line = format!("scene.load(\"{}\")", report.scene_rel);
                    ui.horizontal(|ui| {
                        ui.code(&load_line);
                        if ui.small_button("copy").clicked() {
                            ui.ctx().copy_text(load_line.clone());
                        }
                    });
                }
                ui.add_space(6.0);
                ui.label(RichText::new(&self.status).weak());
            });
    }

    fn door_editor(&mut self, ui: &mut egui::Ui, i: usize) {
        ui.heading("Door");
        let mut remove = false;
        {
            let door = &mut self.doc.doors[i];
            ui.horizontal(|ui| {
                ui.label("name");
                ui.text_edit_singleline(&mut door.name);
            });
            ui.horizontal(|ui| {
                let mut kind_idx = match door.kind {
                    DoorKind::Open => 0,
                    DoorKind::Locked { .. } => 1,
                    DoorKind::Boss => 2,
                };
                let before = kind_idx;
                for (k, label) in DOOR_KINDS.iter().enumerate() {
                    ui.selectable_value(&mut kind_idx, k, *label);
                }
                if kind_idx != before {
                    door.kind = match kind_idx {
                        0 => DoorKind::Open,
                        1 => DoorKind::Locked {
                            key: "small_key".into(),
                        },
                        _ => DoorKind::Boss,
                    };
                }
            });
            if let DoorKind::Locked { key } = &mut door.kind {
                ui.horizontal(|ui| {
                    ui.label("key");
                    ui.text_edit_singleline(key);
                });
            }
            ui.horizontal(|ui| {
                if ui.button("✕ delete door").clicked() {
                    remove = true;
                }
                if ui.button("deselect").clicked() {
                    self.selection = None;
                }
            });
        }
        if remove {
            self.doc.doors.remove(i);
            self.selection = None;
            self.touch(); // doorway geometry goes away
        }
        self.dirty_since_save = true;
    }

    fn entity_editor(&mut self, ui: &mut egui::Ui, i: usize) {
        ui.heading("Entity");
        let mut remove = false;
        {
            let e = &mut self.doc.entities[i];
            ui.horizontal(|ui| {
                ui.label("name");
                ui.text_edit_singleline(&mut e.name);
            });
            ui.horizontal(|ui| {
                ui.label("template");
                ui.text_edit_singleline(&mut e.template);
            });
            let mut tags = e.tags.join(", ");
            ui.horizontal(|ui| {
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
                if ui.button("✕ delete").clicked() {
                    remove = true;
                }
                if ui.button("deselect").clicked() {
                    self.selection = None;
                }
            });
        }
        if remove {
            self.doc.entities.remove(i);
            self.selection = None;
        }
        self.dirty_since_save = true;
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
                if !ctx.egui_wants_keyboard_input() {
                    ctx.input(|i| {
                        for (n, tool) in Tool::ALL.into_iter().enumerate() {
                            let key = [
                                egui::Key::Num1,
                                egui::Key::Num2,
                                egui::Key::Num3,
                                egui::Key::Num4,
                                egui::Key::Num5,
                            ][n];
                            if i.key_pressed(key) {
                                self.tool = tool;
                            }
                        }
                    });
                }

                // ── pick: ray ∩ active storey base plane ─────────────────
                self.hover = response.hover_pos().and_then(|pos| {
                    let u = (pos.x - rect.left()) / rect.width();
                    let v = (pos.y - rect.top()) / rect.height();
                    let (origin, dir) = self.camera.ray_through(u, v, aspect);
                    let plane_y = self.storey as f32 * self.doc.floor_height;
                    if dir.y.abs() < 1e-6 {
                        return None;
                    }
                    let t = (plane_y - origin.y) / dir.y;
                    if t <= 0.0 {
                        return None;
                    }
                    let hit = origin + dir * t;
                    let cell = self.doc.cell_at(self.storey, hit.x, hit.z)?;
                    // nearest edge: which side of the cell centre is closer
                    let centre = self.doc.cell_center(self.storey, cell.0, cell.1);
                    let (dx, dz) = (hit.x - centre[0], hit.z - centre[2]);
                    let across = if dx.abs() > dz.abs() {
                        (cell.0 as i64 + dx.signum() as i64, cell.1 as i64)
                    } else {
                        (cell.0 as i64, cell.1 as i64 + dz.signum() as i64)
                    };
                    Some(Hover {
                        cell,
                        across,
                        world: hit,
                    })
                });

                // ── apply tools ──────────────────────────────────────────
                let painting = response.dragged_by(PointerButton::Primary)
                    || response.clicked_by(PointerButton::Primary);
                if painting && let Some(h) = self.hover {
                    match self.tool {
                        Tool::PaintFloor => {
                            let f = &mut self.doc.floors[self.storey];
                            if f.get(h.cell.0 as i64, h.cell.1 as i64) != Cell::Floor {
                                f.set(h.cell.0, h.cell.1, Cell::Floor);
                                self.touch();
                            }
                        }
                        Tool::EraseFloor => {
                            let f = &mut self.doc.floors[self.storey];
                            if f.get(h.cell.0 as i64, h.cell.1 as i64) != Cell::Empty {
                                f.set(h.cell.0, h.cell.1, Cell::Empty);
                                // drop doors that lost a floor cell
                                let storey = self.storey;
                                self.doc.doors.retain(|d| {
                                    d.floor != storey || (d.a != h.cell && d.b != h.cell)
                                });
                                self.touch();
                            }
                        }
                        // click-tools handled below on `clicked` only
                        _ => {}
                    }
                }
                if response.clicked_by(PointerButton::Primary)
                    && let Some(h) = self.hover
                {
                    match self.tool {
                        Tool::PlaceDoor => self.place_door(h),
                        Tool::PlaceEntity => self.place_entity(h),
                        Tool::Select => self.select_at(h),
                        _ => {}
                    }
                }

                // ── overlay lines ────────────────────────────────────────
                let lines = self.overlay_lines();
                self.scene.set_lines(&lines);

                let params = SceneParams {
                    view_proj: self.camera.view_proj(aspect),
                    eye: self.camera.eye(),
                    brush: glam::Vec4::ZERO,
                    brush_color: [0.0; 4],
                    time: 0.0,
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
            });
    }

    fn place_door(&mut self, h: Hover) {
        if h.across.0 < 0 || h.across.1 < 0 {
            return;
        }
        let across = (h.across.0 as usize, h.across.1 as usize);
        let f = &self.doc.floors[self.storey];
        if f.get(h.cell.0 as i64, h.cell.1 as i64) != Cell::Floor
            || f.get(h.across.0, h.across.1) != Cell::Floor
        {
            self.status = "Doors go on an edge between two floor cells.".into();
            return;
        }
        if self.doc.door_between(self.storey, h.cell, across).is_some() {
            self.status = "There is already a door on that edge.".into();
            return;
        }
        let kind = match self.door_kind_idx {
            0 => DoorKind::Open,
            1 => DoorKind::Locked {
                key: self.door_key.trim().to_string(),
            },
            _ => DoorKind::Boss,
        };
        let name = format!("door_{}", self.doc.doors.len() + 1);
        self.doc.doors.push(Door {
            name,
            floor: self.storey,
            a: h.cell,
            b: across,
            kind,
        });
        self.selection = Some(Selection::Door(self.doc.doors.len() - 1));
        self.touch();
    }

    fn place_entity(&mut self, h: Hover) {
        let template = self.template.trim().to_string();
        let name = if template.is_empty() {
            format!("marker_{}", self.doc.entities.len() + 1)
        } else {
            format!("{template}_{}", self.doc.entities.len() + 1)
        };
        self.doc.entities.push(DungeonEntity {
            name,
            template,
            tags: Vec::new(),
            position: [
                h.world.x,
                self.storey as f32 * self.doc.floor_height,
                h.world.z,
            ],
            yaw_deg: 0.0,
        });
        self.selection = Some(Selection::Entity(self.doc.entities.len() - 1));
        self.dirty_since_save = true;
    }

    fn select_at(&mut self, h: Hover) {
        // nearest entity within 1.5 m, else door on the hovered edge
        let best_entity = self
            .doc
            .entities
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let d = ((e.position[0] - h.world.x).powi(2) + (e.position[2] - h.world.z).powi(2))
                    .sqrt();
                (i, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, d)) = best_entity
            && d < 1.5
        {
            self.selection = Some(Selection::Entity(i));
            return;
        }
        if h.across.0 >= 0 && h.across.1 >= 0 {
            let across = (h.across.0 as usize, h.across.1 as usize);
            if let Some(door) = self.doc.door_between(self.storey, h.cell, across) {
                let idx = self
                    .doc
                    .doors
                    .iter()
                    .position(|d| d.name == door.name)
                    .unwrap();
                self.selection = Some(Selection::Door(idx));
                return;
            }
        }
        self.selection = None;
    }

    fn overlay_lines(&self) -> Vec<LineVertex> {
        let mut lines = Vec::new();
        let doc = &self.doc;
        let f = &doc.floors[self.storey];
        let cs = doc.cell_size;
        let (ox, oz) = (doc.origin_x(), doc.origin_z());
        let y = self.storey as f32 * doc.floor_height + 0.03;

        // grid over the active storey
        let grid_c = [0.35, 0.38, 0.45, 0.5];
        for x in 0..=f.width {
            let wx = ox + x as f32 * cs;
            lines.push(LineVertex::new(Vec3::new(wx, y, oz), grid_c));
            lines.push(LineVertex::new(
                Vec3::new(wx, y, oz + f.depth as f32 * cs),
                grid_c,
            ));
        }
        for z in 0..=f.depth {
            let wz = oz + z as f32 * cs;
            lines.push(LineVertex::new(Vec3::new(ox, y, wz), grid_c));
            lines.push(LineVertex::new(
                Vec3::new(ox + f.width as f32 * cs, y, wz),
                grid_c,
            ));
        }

        // hovered cell / edge highlight
        if let Some(h) = self.hover {
            let (x0, z0) = (ox + h.cell.0 as f32 * cs, oz + h.cell.1 as f32 * cs);
            let c = match self.tool {
                Tool::EraseFloor => [1.0, 0.4, 0.3, 0.9],
                Tool::PlaceDoor => [0.4, 0.9, 1.0, 0.9],
                _ => [1.0, 1.0, 1.0, 0.8],
            };
            if self.tool == Tool::PlaceDoor {
                // highlight the edge toward `across`
                let (dx, dz) = (
                    (h.across.0 - h.cell.0 as i64) as f32,
                    (h.across.1 - h.cell.1 as i64) as f32,
                );
                let (ex0, ez0, ex1, ez1) = if dx > 0.0 {
                    (x0 + cs, z0, x0 + cs, z0 + cs)
                } else if dx < 0.0 {
                    (x0, z0, x0, z0 + cs)
                } else if dz > 0.0 {
                    (x0, z0 + cs, x0 + cs, z0 + cs)
                } else {
                    (x0, z0, x0 + cs, z0)
                };
                lines.push(LineVertex::new(Vec3::new(ex0, y + 0.05, ez0), c));
                lines.push(LineVertex::new(Vec3::new(ex1, y + 0.05, ez1), c));
            } else {
                for (a, b) in [
                    ((x0, z0), (x0 + cs, z0)),
                    ((x0 + cs, z0), (x0 + cs, z0 + cs)),
                    ((x0 + cs, z0 + cs), (x0, z0 + cs)),
                    ((x0, z0 + cs), (x0, z0)),
                ] {
                    lines.push(LineVertex::new(Vec3::new(a.0, y + 0.05, a.1), c));
                    lines.push(LineVertex::new(Vec3::new(b.0, y + 0.05, b.1), c));
                }
            }
        }

        // doors on the active storey
        for (i, door) in doc.doors.iter().enumerate() {
            if door.floor != self.storey {
                continue;
            }
            let ca = doc.cell_center(door.floor, door.a.0, door.a.1);
            let cb = doc.cell_center(door.floor, door.b.0, door.b.1);
            let mid = Vec3::new((ca[0] + cb[0]) * 0.5, y, (ca[2] + cb[2]) * 0.5);
            let selected = self.selection == Some(Selection::Door(i));
            let c = if selected {
                [1.0, 0.8, 0.2, 1.0]
            } else {
                match door.kind {
                    DoorKind::Open => [0.4, 0.9, 0.5, 1.0],
                    DoorKind::Locked { .. } => [0.95, 0.75, 0.2, 1.0],
                    DoorKind::Boss => [0.95, 0.3, 0.3, 1.0],
                }
            };
            // diamond marker at the edge midpoint + post up to door height
            let s = cs * 0.25;
            for (a, b) in [
                (
                    Vec3::new(mid.x - s, mid.y, mid.z),
                    Vec3::new(mid.x, mid.y, mid.z - s),
                ),
                (
                    Vec3::new(mid.x, mid.y, mid.z - s),
                    Vec3::new(mid.x + s, mid.y, mid.z),
                ),
                (
                    Vec3::new(mid.x + s, mid.y, mid.z),
                    Vec3::new(mid.x, mid.y, mid.z + s),
                ),
                (
                    Vec3::new(mid.x, mid.y, mid.z + s),
                    Vec3::new(mid.x - s, mid.y, mid.z),
                ),
            ] {
                lines.push(LineVertex::new(a, c));
                lines.push(LineVertex::new(b, c));
            }
            lines.push(LineVertex::new(mid, c));
            lines.push(LineVertex::new(mid + Vec3::Y * doc.door_height, c));
        }

        // entities on the active storey band
        for (i, e) in doc.entities.iter().enumerate() {
            let base = self.storey as f32 * doc.floor_height;
            if e.position[1] < base - 0.5 || e.position[1] > base + doc.wall_height {
                continue;
            }
            let p = Vec3::from_array(e.position);
            let selected = self.selection == Some(Selection::Entity(i));
            let c = if selected {
                [1.0, 0.8, 0.2, 1.0]
            } else {
                [0.3, 0.8, 1.0, 0.95]
            };
            let top = p + Vec3::Y * 1.6;
            lines.push(LineVertex::new(p, c));
            lines.push(LineVertex::new(top, c));
            let s = 0.3;
            for (a, b) in [
                (Vec3::X * s, Vec3::Z * s),
                (Vec3::Z * s, -Vec3::X * s),
                (-Vec3::X * s, -Vec3::Z * s),
                (-Vec3::Z * s, Vec3::X * s),
            ] {
                lines.push(LineVertex::new(top + a, c));
                lines.push(LineVertex::new(top + b, c));
            }
        }

        lines
    }

    fn new_dungeon_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = &mut self.new_dialog else {
            return;
        };
        let mut create = false;
        let mut cancel = false;
        egui::Window::new("New dungeon")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut d.name);
                });
                ui.horizontal(|ui| {
                    ui.label("Grid");
                    ui.add(egui::DragValue::new(&mut d.width).range(8..=128));
                    ui.label("×");
                    ui.add(egui::DragValue::new(&mut d.depth).range(8..=128));
                    ui.label("cells");
                });
                ui.horizontal(|ui| {
                    ui.label("Cell size (m)");
                    ui.add(
                        egui::DragValue::new(&mut d.cell_size)
                            .range(1.0..=8.0)
                            .speed(0.1),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Wall height (m)");
                    ui.add(
                        egui::DragValue::new(&mut d.wall_height)
                            .range(2.0..=12.0)
                            .speed(0.1),
                    );
                });
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
            let mut doc = DungeonDoc::new(&d.name, d.width, d.depth);
            doc.cell_size = d.cell_size;
            doc.wall_height = d.wall_height;
            doc.floor_height = d.wall_height + 1.0;
            self.camera = OrbitCamera::for_island(d.width as f32 * d.cell_size);
            self.camera.pitch = -1.1;
            self.doc = doc;
            self.doc_path = None;
            self.storey = 0;
            self.selection = None;
            self.last_export = None;
            self.touch();
        } else if cancel {
            self.new_dialog = None;
        }
    }

    // ── io ────────────────────────────────────────────────────────────────

    fn save_project(&mut self, force_dialog: bool) {
        let path = if force_dialog || self.doc_path.is_none() {
            rfd::FileDialog::new()
                .set_title("Save dungeon project")
                .set_file_name(format!("{}.wrightdungeon", self.doc.name))
                .save_file()
        } else {
            self.doc_path.clone()
        };
        let Some(path) = path else { return };
        match self.doc.save(&path) {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                self.doc_path = Some(path);
                self.dirty_since_save = false;
            }
            Err(e) => self.status = format!("Save failed: {e:#}"),
        }
    }

    fn open_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open dungeon project")
            .add_filter("wright dungeon", &["wrightdungeon", "toml"])
            .pick_file()
        else {
            return;
        };
        match DungeonDoc::load(&path) {
            Ok(doc) => {
                self.camera = OrbitCamera::for_island(doc.floors[0].width as f32 * doc.cell_size);
                self.camera.pitch = -1.1;
                self.doc = doc;
                self.doc_path = Some(path);
                self.storey = 0;
                self.selection = None;
                self.last_export = None;
                self.dirty_since_save = false;
                self.needs_remesh = true;
            }
            Err(e) => self.status = format!("Open failed: {e:#}"),
        }
    }
}
