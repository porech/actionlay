//! Visual layout drafts. Player state changes only after a successful save.
use crate::video_view::{VideoView, fit_rect};
use actionlay_layout::geom::{self, Anchor, Aspect, Rect};
use actionlay_layout::{Layout, Node, Widget};
use actionlay_render::{Renderer, tiny_skia::Pixmap};
use actionlay_telemetry::{Metric, Telemetry};
use eframe::egui;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

type NodePath = Vec<usize>;

#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Save,
    SaveAs,
    Exit,
    New,
}

struct Drag {
    start: egui::Pos2,
    path: NodePath,
    before: Layout,
    rect: Rect,
    parent: Rect,
    resize: bool,
}

pub struct Editor {
    pub draft: Layout,
    saved: Layout,
    pub path: Option<PathBuf>,
    is_new: bool,
    selection: Option<NodePath>,
    properties_buffer: Option<(NodePath, Value)>,
    invalid_properties: bool,
    property_error: Option<String>,
    undo: Vec<Layout>,
    redo: Vec<Layout>,
    clipboard: Option<Node>,
    drag: Option<Drag>,
    pub video_background: bool,
    pub dimensions: [u32; 2],
    snap: bool,
    renderer: Renderer,
    offline_maps: actionlay_maps::TileStore,
    maps: actionlay_maps::TileStore,
    demo: Telemetry,
    texture: Option<egui::TextureHandle>,
    last_preview: Option<String>,
    background: Option<egui::TextureHandle>,
    schema: Value,
    pub error: Option<String>,
}

impl Editor {
    pub fn new(
        layout: Layout,
        path: Option<PathBuf>,
        video_size: Option<[u32; 2]>,
        is_new: bool,
    ) -> Self {
        Self {
            saved: layout.clone(),
            draft: layout,
            path,
            is_new,
            selection: None,
            properties_buffer: None,
            invalid_properties: false,
            property_error: None,
            undo: Vec::new(),
            redo: Vec::new(),
            clipboard: None,
            drag: None,
            video_background: video_size.is_some(),
            dimensions: video_size.unwrap_or([1920, 1080]),
            snap: true,
            renderer: Renderer::new(),
            offline_maps: actionlay_maps::TileStore::offline(),
            maps: actionlay_maps::TileStore::offline(),
            demo: Telemetry::preview(),
            texture: None,
            last_preview: None,
            background: None,
            schema: actionlay_layout::json_schema(),
            error: None,
        }
    }

    pub fn blank() -> Layout {
        Layout {
            schema: None,
            version: actionlay_layout::CURRENT_VERSION,
            name: Some("Untitled".into()),
            design_aspect: Some(Aspect::WIDESCREEN),
            units: None,
            theme: None,
            nodes: Vec::new(),
            extra: Default::default(),
        }
    }

    pub fn dirty(&self) -> bool {
        self.is_new || self.draft != self.saved || self.invalid_properties
    }

    pub fn is_system_copy(&self) -> bool {
        !self.is_new && self.path.is_none()
    }

    pub fn set_maps(&mut self, maps: actionlay_maps::TileStore) {
        self.maps = maps;
    }

    fn commit(&mut self, before: Layout) {
        if before != self.draft {
            self.undo.push(before);
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        if self.invalid_properties {
            return Err("Fix invalid widget parameters before saving".into());
        }
        // Validate first, then atomically replace the destination. Neither the
        // saved baseline nor the destination changes on validation/write errors.
        let text = self.draft.to_json().map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        file.write_all((text + "\n").as_bytes())
            .and_then(|_| file.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        file.persist(path).map_err(|e| e.to_string())?;
        self.saved = self.draft.clone();
        self.path = Some(path.to_path_buf());
        self.is_new = false;
        self.error = None;
        Ok(())
    }

    fn undo(&mut self) {
        if let Some(layout) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.draft, layout));
            self.selection = None;
            self.properties_buffer = None;
            self.invalid_properties = false;
            self.property_error = None;
        }
    }
    fn redo(&mut self) {
        if let Some(layout) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.draft, layout));
            self.selection = None;
            self.properties_buffer = None;
            self.invalid_properties = false;
            self.property_error = None;
        }
    }

    fn add(&mut self, kind: &str, position: [f32; 2], root: Rect) {
        let before = self.draft.clone();
        let mut node = template(kind);
        set_relative_position(
            &mut node,
            root,
            Rect::new(position[0], position[1], 0.0, 0.0),
        );
        let index = self.draft.nodes.len();
        self.draft.nodes.push(node);
        self.selection = Some(vec![index]);
        self.commit(before);
    }

    fn duplicate(&mut self) {
        if let Some(node) = self
            .selection
            .as_ref()
            .and_then(|p| node_at(&self.draft.nodes, p))
            .cloned()
        {
            self.insert_copy(node);
        }
    }
    fn insert_copy(&mut self, mut node: Node) {
        let before = self.draft.clone();
        strip_ids(&mut node);
        let mut v = serde_json::to_value(&node).unwrap();
        let offset = v.get("offset").cloned().unwrap_or(json!([0.0, 0.0]));
        v["offset"] = json!([
            offset[0].as_f64().unwrap_or(0.0) + 24.0,
            offset[1].as_f64().unwrap_or(0.0) + 24.0
        ]);
        node = serde_json::from_value(v).unwrap();
        let path = self
            .selection
            .as_ref()
            .map(|p| p[..p.len() - 1].to_vec())
            .unwrap_or_default();
        if let Some(nodes) = children_at_mut(&mut self.draft.nodes, &path) {
            let index = nodes.len();
            nodes.push(node);
            self.selection = Some(path.into_iter().chain([index]).collect());
            self.commit(before);
        }
    }
    fn delete(&mut self) {
        self.properties_buffer = None;
        self.invalid_properties = false;
        self.property_error = None;
        if let Some(path) = self.selection.take() {
            let before = self.draft.clone();
            if let Some(nodes) = children_at_mut(&mut self.draft.nodes, &path[..path.len() - 1]) {
                nodes.remove(*path.last().unwrap());
                self.commit(before);
            }
        }
    }
    fn reorder(&mut self, forward: bool) {
        if let Some(path) = &mut self.selection {
            let before = self.draft.clone();
            let i = *path.last().unwrap();
            if let Some(nodes) = children_at_mut(&mut self.draft.nodes, &path[..path.len() - 1]) {
                let j = if forward { i + 1 } else { i.saturating_sub(1) };
                if j < nodes.len() && j != i {
                    nodes.swap(i, j);
                    *path.last_mut().unwrap() = j;
                }
            }
            self.commit(before);
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        video: Option<&mut VideoView>,
        video_size: Option<[u32; 2]>,
        telemetry: Option<&Telemetry>,
        time: f64,
    ) -> Option<Action> {
        let ctx = ui.ctx().clone();
        if video_size.is_none() {
            self.video_background = false;
        }
        let warnings_telemetry = if self.video_background {
            telemetry
        } else {
            None
        };
        let mut action = None;
        egui::Panel::top("editor-toolbar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("Edit layout");
                ui.label(format!(
                    "{}{}",
                    self.draft.name.as_deref().unwrap_or("Untitled"),
                    if self.dirty() { " *" } else { "" }
                ));
                if ui.button("New layout…").clicked() {
                    action = Some(Action::New);
                }
                if ui.button("Save").clicked() {
                    action = Some(Action::Save);
                }
                if ui.button("Save as…").clicked() {
                    action = Some(Action::SaveAs);
                }
                if ui.button("Exit editor").clicked() {
                    action = Some(Action::Exit);
                }
                if ui
                    .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                    .clicked()
                {
                    self.redo();
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Background");
                ui.add_enabled_ui(video_size.is_some(), |ui| {
                    ui.selectable_value(&mut self.video_background, true, "Open video");
                });
                ui.selectable_value(&mut self.video_background, false, "Static image");
                if !self.video_background {
                    egui::ComboBox::from_id_salt("preview-aspect")
                        .selected_text(format!("{} × {}", self.dimensions[0], self.dimensions[1]))
                        .show_ui(ui, |ui| {
                            for (name, size) in [
                                ("16:9 · Full HD", [1920, 1080]),
                                ("16:9 · 4K", [3840, 2160]),
                                ("4:3", [1440, 1080]),
                                ("9:16", [1080, 1920]),
                                ("1:1", [1080, 1080]),
                            ] {
                                ui.selectable_value(&mut self.dimensions, size, name);
                            }
                        });
                    for n in &mut self.dimensions {
                        ui.add(egui::DragValue::new(n).range(64..=16384));
                    }
                }
                ui.checkbox(&mut self.snap, "Snap to guides");
            });
        });
        if ui.is_enabled() && !ctx.egui_wants_keyboard_input() {
            ctx.input_mut(|i| {
                if i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::S,
                ) {
                    action = Some(Action::SaveAs);
                } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::S) {
                    action = Some(Action::Save);
                }
                if i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                ) {
                    self.redo();
                } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z) {
                    self.undo();
                }
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::D) {
                    self.duplicate();
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
                {
                    self.delete();
                }
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::C) {
                    self.clipboard = self
                        .selection
                        .as_ref()
                        .and_then(|p| node_at(&self.draft.nodes, p))
                        .cloned();
                }
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::V)
                    && let Some(n) = self.clipboard.clone()
                {
                    self.insert_copy(n);
                }
            });
        }
        egui::Panel::left("editor-palette")
            .default_size(190.0)
            .resizable(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Widgets");
                    ui.small("Drag onto the preview, or click to add.");
                    for kind in Widget::TYPES {
                        ui.horizontal(|ui| {
                            ui.dnd_drag_source(
                                egui::Id::new(("palette", kind)),
                                kind.to_owned(),
                                |ui| {
                                    ui.label(kind.replace('_', " "));
                                },
                            );
                            if ui.small_button("+").clicked() {
                                let size = if self.video_background {
                                    video_size.unwrap_or(self.dimensions)
                                } else {
                                    self.dimensions
                                };
                                let mode =
                                    crate::layouts::scale_mode_for(size[0], size[1], &self.draft);
                                let scale = geom::scale_factor(
                                    mode,
                                    size[0] as f32,
                                    size[1] as f32,
                                    self.draft
                                        .design_aspect
                                        .unwrap_or(Aspect::WIDESCREEN)
                                        .ratio(),
                                );
                                self.add(
                                    kind,
                                    [80.0, 80.0],
                                    geom::root_box(size[0] as f32, size[1] as f32, scale),
                                );
                            }
                        });
                    }
                    ui.separator();
                    ui.heading("Layers");
                    layer_tree(
                        ui,
                        &self.draft.nodes,
                        &mut Vec::new(),
                        &mut self.selection,
                        warnings_telemetry,
                    );
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Duplicate").clicked() {
                            self.duplicate();
                        }
                        if ui.button("Delete").clicked() {
                            self.delete();
                        }
                        if ui.button("Forward").clicked() {
                            self.reorder(true);
                        }
                        if ui.button("Backward").clicked() {
                            self.reorder(false);
                        }
                    });
                });
            });
        egui::Panel::right("editor-properties")
            .default_size(290.0)
            .resizable(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Properties");
                    self.properties(ui, warnings_telemetry);
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
            if !self.video_background {
                ui.horizontal(|ui| {
                    if ui.button("Load background image…").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"])
                            .pick_file()
                    {
                        match load_background(&ctx, &path) {
                            Ok(t) => self.background = Some(t),
                            Err(e) => self.error = Some(e),
                        }
                    }
                    if self.background.is_some() && ui.button("Clear image").clicked() {
                        self.background = None;
                    }
                    ui.small("Demonstration data · map downloads disabled");
                });
            }
            for error in [&self.error, &self.property_error].into_iter().flatten() {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            let size = if self.video_background {
                video_size.unwrap_or(self.dimensions)
            } else {
                self.dimensions
            };
            let canvas = fit_rect(ui.available_rect_before_wrap(), size[0], size[1]);
            if canvas.width() < 8.0 || canvas.height() < 8.0 {
                ui.label("Enlarge the window or narrow the side panels to show the preview.");
                return;
            }
            let response = ui.allocate_rect(canvas, egui::Sense::click_and_drag());
            let mode = crate::layouts::scale_mode_for(
                size[0],
                size[1],
                self.drag.as_ref().map_or(&self.draft, |d| &d.before),
            );
            let scale = geom::scale_factor(
                mode,
                canvas.width(),
                canvas.height(),
                self.draft
                    .design_aspect
                    .unwrap_or(Aspect::WIDESCREEN)
                    .ratio(),
            );
            let root = geom::root_box(canvas.width(), canvas.height(), scale);
            if self.video_background {
                if let Some(video) = video {
                    video.show(ui, canvas, Some(canvas), false);
                }
            } else {
                ui.painter()
                    .rect_filled(canvas, 0.0, egui::Color32::from_rgb(35, 43, 50));
                if let Some(background) = &self.background {
                    ui.painter().image(
                        background.id(),
                        canvas,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
            }
            // Limit the editor preview texture, not the canvas or layout geometry.
            let ppp = ctx.pixels_per_point().min(2.0);
            let ratio = (2048.0 / (canvas.width() * ppp).max(canvas.height() * ppp)).min(1.0);
            let width = (canvas.width() * ppp * ratio).round().max(1.0) as u32;
            let height = (canvas.height() * ppp * ratio).round().max(1.0) as u32;
            let tel = if self.video_background {
                telemetry.unwrap_or(&self.demo)
            } else {
                &self.demo
            };
            let t = if self.video_background { time } else { 30.0 };
            let maps = if self.video_background && telemetry.is_some() {
                &self.maps
            } else {
                &self.offline_maps
            };
            let key = format!(
                "{}:{width}:{height}:{t}:{}:{mode:?}:{}",
                serde_json::to_string(&self.draft).unwrap(),
                tel.identity(),
                maps.revision()
            );
            if self.last_preview.as_ref() != Some(&key) {
                self.renderer.set_scale_mode(mode);
                self.renderer.set_maps(maps.clone());
                if let Some(mut pixmap) = Pixmap::new(width, height) {
                    self.renderer
                        .render_editor_into(&self.draft, tel, t, &mut pixmap);
                    let image = egui::ColorImage::from_rgba_premultiplied(
                        [width as usize, height as usize],
                        pixmap.data(),
                    );
                    if let Some(texture) = &mut self.texture {
                        texture.set(image, egui::TextureOptions::LINEAR);
                    } else {
                        self.texture = Some(ctx.load_texture(
                            "editor-preview",
                            image,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    self.last_preview = Some(key);
                }
            }
            if let Some(texture) = &self.texture {
                ui.painter().image(
                    texture.id(),
                    canvas,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            self.canvas_input(ui, &response, canvas, root, scale);
        });
        action
    }

    fn canvas_input(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        canvas: egui::Rect,
        root: Rect,
        scale: f32,
    ) {
        let ctx = ui.ctx().clone();
        if let Some(kind) = response.dnd_release_payload::<String>()
            && let Some(pos) = ctx.pointer_interact_pos()
        {
            let point = (pos - canvas.min) / scale;
            self.add(&kind, [point.x, point.y], root);
        }
        let boxes = self.renderer.hit_boxes().to_vec();
        let screen = |r: Rect| {
            egui::Rect::from_min_size(
                canvas.min + egui::vec2(r.x * scale, r.y * scale),
                egui::vec2(r.w * scale, r.h * scale),
            )
        };
        let selected = self
            .selection
            .as_ref()
            .and_then(|p| boxes.iter().find(|b| &b.path == p));
        let mut resize_response = None;
        if let Some(hit) = selected {
            let rect = screen(hit.rect);
            ui.painter().rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.5, egui::Color32::LIGHT_BLUE),
                egui::StrokeKind::Outside,
            );
            let handle = egui::Rect::from_center_size(rect.right_bottom(), egui::vec2(12.0, 12.0));
            ui.painter()
                .rect_filled(handle, 2.0, egui::Color32::LIGHT_BLUE);
            resize_response = Some(
                ui.interact(handle, egui::Id::new("widget-resize"), egui::Sense::drag())
                    .on_hover_cursor(egui::CursorIcon::ResizeNwSe),
            );
            if !canvas.contains_rect(rect) {
                ui.painter().text(
                    canvas.left_top() + egui::vec2(8.0, 8.0),
                    egui::Align2::LEFT_TOP,
                    "Selected widget extends outside the frame",
                    egui::FontId::proportional(13.0),
                    egui::Color32::YELLOW,
                );
            }
        }
        let resizing = resize_response.as_ref().is_some_and(|r| r.drag_started());
        if resizing || response.drag_started() || response.clicked() {
            let hit = if resizing
                || (response.drag_started()
                    && selected.is_some_and(|b| {
                        ctx.pointer_interact_pos()
                            .is_some_and(|pos| screen(b.rect).contains(pos))
                    })) {
                selected.cloned()
            } else {
                ctx.pointer_interact_pos().and_then(|pos| {
                    boxes
                        .iter()
                        .rev()
                        .find(|b| screen(b.rect).contains(pos))
                        .cloned()
                })
            };
            if let Some(hit) = hit {
                self.selection = Some(hit.path.clone());
                if resizing || response.drag_started() {
                    self.drag = Some(Drag {
                        start: ctx
                            .input(|i| i.pointer.press_origin())
                            .unwrap_or_else(|| ctx.pointer_interact_pos().unwrap_or_default()),
                        path: hit.path,
                        before: self.draft.clone(),
                        rect: hit.rect,
                        parent: hit.parent,
                        resize: resizing,
                    });
                }
            } else if response.clicked() {
                self.selection = None;
            }
        }
        let delta = self
            .drag
            .as_ref()
            .and_then(|d| ctx.input(|i| i.pointer.latest_pos()).map(|p| p - d.start))
            .unwrap_or_default();
        if let Some(drag) = &self.drag {
            self.properties_buffer = None;
            let mut layout = drag.before.clone();
            if let Some(node) = node_at_mut(&mut layout.nodes, &drag.path) {
                if drag.resize {
                    resize_node(node, drag.rect, delta / scale, drag.parent);
                } else {
                    let mut rect = drag.rect;
                    rect.x += delta.x / scale;
                    rect.y += delta.y / scale;
                    if self.snap && !ctx.input(|i| i.modifiers.alt) {
                        snap_rect(&mut rect, root, 8.0 / scale);
                    }
                    set_relative_position(node, drag.parent, rect);
                    let point = anchor_point(node, drag.parent);
                    let anchor = canvas.min + egui::vec2(point[0] * scale, point[1] * scale);
                    ui.painter().line_segment(
                        [anchor, screen(rect).center()],
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 125, 160)),
                    );
                }
            }
            if layout.to_json().is_ok() {
                self.draft = layout;
            }
        }
        if ctx.input(|i| i.pointer.any_released())
            && let Some(drag) = self.drag.take()
        {
            self.commit(drag.before);
        }
    }

    fn properties(&mut self, ui: &mut egui::Ui, telemetry: Option<&Telemetry>) {
        let before = self.draft.clone();
        if let Some(path) = self.selection.clone()
            && let Some(node) = node_at(&self.draft.nodes, &path)
        {
            ui.strong(node.type_name().replace('_', " "));
            for warning in metric_warnings(node, telemetry) {
                ui.colored_label(egui::Color32::YELLOW, warning);
            }
            let mut value = self
                .properties_buffer
                .as_ref()
                .filter(|(p, _)| *p == path)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| serde_json::to_value(node).unwrap());
            let node_schema = self.schema["$defs"]["Node"]["oneOf"]
                .as_array()
                .and_then(|nodes| {
                    nodes
                        .iter()
                        .find(|n| n["properties"]["type"]["const"] == value["type"])
                })
                .and_then(|s| s["$ref"].as_str())
                .and_then(|r| self.schema.pointer(&r[1..]))
                .cloned();
            if let Some(schema) = node_schema {
                if ui.button("Reset widget parameters").clicked() {
                    let template = template(node.type_name());
                    let mut fresh = serde_json::to_value(template).unwrap();
                    for key in [
                        "id",
                        "name",
                        "anchor",
                        "offset",
                        "offset_relative",
                        "children",
                    ] {
                        if let Some(v) = value.get(key) {
                            fresh[key] = v.clone();
                        }
                    }
                    // Unknown fields remain intact.
                    if let Some(properties) = schema["properties"].as_object() {
                        for (key, v) in value.as_object().unwrap() {
                            if !properties.contains_key(key) && key != "type" {
                                fresh[key] = v.clone();
                            }
                        }
                    }
                    value = fresh;
                }
                property_object(ui, &mut value, &schema, &self.schema, 0);
                self.properties_buffer = Some((path.clone(), value.clone()));
                match serde_json::from_value::<Node>(value) {
                    Ok(node) => {
                        let mut proposed = self.draft.clone();
                        *node_at_mut(&mut proposed.nodes, &path).unwrap() = node;
                        match proposed.to_json() {
                            Ok(_) => {
                                self.draft = proposed;
                                self.property_error = None;
                                self.invalid_properties = false;
                            }
                            Err(e) => {
                                self.property_error = Some(e.to_string());
                                self.invalid_properties = true;
                            }
                        }
                    }
                    Err(e) => {
                        self.property_error = Some(e.to_string());
                        self.invalid_properties = true;
                    }
                }
            } else {
                ui.weak("This widget is preserved but is not supported by this version.");
            }
        } else {
            ui.label("Layout name");
            let name = self.draft.name.get_or_insert_default();
            ui.text_edit_singleline(name);
            ui.small("Select a widget in the preview or in Layers to edit it.");
            ui.label("Design proportions");
            let aspect = self.draft.design_aspect.get_or_insert(Aspect::WIDESCREEN);
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut aspect.w).range(1.0..=16384.0));
                ui.label(":");
                ui.add(egui::DragValue::new(&mut aspect.h).range(1.0..=16384.0));
            });
            let mut value = serde_json::to_value(&self.draft).unwrap();
            let schema = json!({"properties":{"theme":{"anyOf":[{"$ref":"#/$defs/Theme"},{"type":"null"}]},"units":{"anyOf":[{"$ref":"#/$defs/Units"},{"type":"null"}]}}});
            property_object(ui, &mut value, &schema, &self.schema, 0);
            if let Ok(layout) = serde_json::from_value::<Layout>(value)
                && layout.to_json().is_ok()
            {
                self.draft = layout;
            }
        }
        if before != self.draft {
            self.commit(before);
        }
    }
}

fn template(kind: &str) -> Node {
    let mut v = json!({"type":kind});
    match kind {
        "group" | "frame" => {
            v["size"] = json!([400, 240]);
            v["children"] = json!([]);
        }
        "text" => v["text"] = json!("Text"),
        "metric" | "metric_unit" | "bar" | "zone_bar" | "gauge" | "chart" => {
            v["metric"] = json!("speed")
        }
        "gradient_chart" => v["metric"] = json!("alt"),
        "compass" => v["metric"] = json!("heading"),
        "icon" => v["icon"] = json!("speed"),
        _ => {}
    }
    serde_json::from_value(v).expect("every palette template is a valid node")
}

pub fn node_at<'a>(nodes: &'a [Node], path: &[usize]) -> Option<&'a Node> {
    let (&i, tail) = path.split_first()?;
    let node = nodes.get(i)?;
    if tail.is_empty() {
        Some(node)
    } else {
        match node {
            Node::Known(w) => node_at(w.children(), tail),
            _ => None,
        }
    }
}
fn children_at_mut<'a>(nodes: &'a mut Vec<Node>, path: &[usize]) -> Option<&'a mut Vec<Node>> {
    if path.is_empty() {
        return Some(nodes);
    }
    let (&i, tail) = path.split_first()?;
    match nodes.get_mut(i)? {
        Node::Known(Widget::Group(g)) => children_at_mut(&mut g.children, tail),
        Node::Known(Widget::Frame(f)) => children_at_mut(&mut f.children, tail),
        _ => None,
    }
}
fn node_at_mut<'a>(nodes: &'a mut [Node], path: &[usize]) -> Option<&'a mut Node> {
    let (&i, tail) = path.split_first()?;
    if tail.is_empty() {
        nodes.get_mut(i)
    } else {
        match nodes.get_mut(i)? {
            Node::Known(Widget::Group(g)) => node_at_mut(&mut g.children, tail),
            Node::Known(Widget::Frame(f)) => node_at_mut(&mut f.children, tail),
            _ => None,
        }
    }
}
fn strip_ids(node: &mut Node) {
    let mut value = serde_json::to_value(&*node).unwrap();
    fn strip(v: &mut Value) {
        if let Some(o) = v.as_object_mut() {
            o.remove("id");
            if let Some(Value::Array(children)) = o.get_mut("children") {
                for child in children {
                    strip(child);
                }
            }
        }
    }
    strip(&mut value);
    *node = serde_json::from_value(value).unwrap();
}
fn set_relative_position(node: &mut Node, parent: Rect, rect: Rect) {
    let mut v = serde_json::to_value(&*node).unwrap();
    let anchor = v
        .get("anchor")
        .cloned()
        .and_then(|v| serde_json::from_value::<Anchor>(v).ok())
        .unwrap_or_default();
    let (fx, fy) = anchor.fractions();
    v["offset"] = json!([0.0, 0.0]);
    v["offset_relative"] = json!([
        (rect.x + rect.w * fx - parent.x - parent.w * fx) / parent.w.max(1.0),
        (rect.y + rect.h * fy - parent.y - parent.h * fy) / parent.h.max(1.0)
    ]);
    // Zero-sized upstream groups have no relative coordinate system.
    if parent.w <= 0.0 || parent.h <= 0.0 {
        v["offset"] = json!([
            rect.x + rect.w * fx - parent.x,
            rect.y + rect.h * fy - parent.y
        ]);
        v.as_object_mut().unwrap().remove("offset_relative");
    }
    *node = serde_json::from_value(v).unwrap();
}
fn anchor_point(node: &Node, parent: Rect) -> [f32; 2] {
    let a = match node {
        Node::Known(w) => w.common().anchor.unwrap_or_default(),
        _ => Anchor::default(),
    };
    let (fx, fy) = a.fractions();
    [parent.x + parent.w * fx, parent.y + parent.h * fy]
}
fn resize_node(node: &mut Node, rect: Rect, delta: egui::Vec2, parent: Rect) {
    let mut v = serde_json::to_value(&*node).unwrap();
    let mut width = (rect.w + delta.x).max(8.0);
    let mut height = (rect.h + delta.y).max(8.0);
    let kind = node.type_name();
    if matches!(kind, "compass" | "gauge" | "g_meter") {
        width = width.max(height);
        height = width;
        v["diameter"] = json!(width);
    } else if matches!(
        kind,
        "icon" | "gps_lock_icon" | "text" | "metric" | "metric_unit" | "datetime"
    ) {
        let factor = (width / rect.w.max(1.0)).max(height / rect.h.max(1.0));
        width = rect.w * factor;
        height = rect.h * factor;
        let old = v["size"].as_f64().unwrap_or(rect.h as f64);
        v["size"] = json!((old as f32 * factor).max(1.0));
    } else if kind == "map" {
        let factor = (width / rect.w.max(1.0)).max(height / rect.h.max(1.0));
        width = rect.w * factor;
        height = rect.h * factor;
        v["size"] = json!([width, height]);
    } else {
        v["size"] = json!([width, height]);
    }
    if let Ok(mut updated) = serde_json::from_value::<Node>(v) {
        set_relative_position(
            &mut updated,
            parent,
            Rect::new(rect.x, rect.y, width, height),
        );
        *node = updated;
    }
}
fn snap_rect(rect: &mut Rect, root: Rect, tolerance: f32) {
    for (pos, length, origin, total) in [
        (&mut rect.x, rect.w, root.x, root.w),
        (&mut rect.y, rect.h, root.y, root.h),
    ] {
        for target in [
            origin,
            origin + total / 2.0 - length / 2.0,
            origin + total - length,
        ] {
            if (*pos - target).abs() < tolerance {
                *pos = target;
                break;
            }
        }
    }
}
fn metric_warnings(node: &Node, telemetry: Option<&Telemetry>) -> Vec<String> {
    let (Node::Known(w), Some(t)) = (node, telemetry) else {
        return Vec::new();
    };
    w.required_metrics()
        .iter()
        .filter_map(|id| {
            if *id == "timestamp" {
                return t
                    .start_utc()
                    .is_none()
                    .then(|| "Time of day is not in this video".into());
            }
            let metric = Metric::from_id(id)?;
            let coverage = t.availability().coverage(metric);
            if coverage == 0.0 {
                Some(format!("{id} is not available in this video"))
            } else if coverage < 0.99 {
                Some(format!(
                    "{id}: {:.0}% coverage in loaded telemetry",
                    coverage * 100.0
                ))
            } else {
                None
            }
        })
        .collect()
}
fn layer_tree(
    ui: &mut egui::Ui,
    nodes: &[Node],
    path: &mut NodePath,
    selected: &mut Option<NodePath>,
    telemetry: Option<&Telemetry>,
) {
    for (i, node) in nodes.iter().enumerate() {
        path.push(i);
        let name = match node {
            Node::Known(w) => w
                .common()
                .name
                .as_deref()
                .or(node.id())
                .unwrap_or(node.type_name()),
            _ => node.type_name(),
        };
        let warnings = metric_warnings(node, telemetry);
        let label = format!("{}{}", if warnings.is_empty() { "" } else { "⚠ " }, name);
        if ui
            .selectable_label(selected.as_ref() == Some(path), label)
            .on_hover_text(warnings.join("\n"))
            .clicked()
        {
            *selected = Some(path.clone());
        }
        if let Node::Known(w) = node
            && !w.children().is_empty()
        {
            ui.indent(egui::Id::new(path.clone()), |ui| {
                layer_tree(ui, w.children(), path, selected, telemetry)
            });
        }
        path.pop();
    }
}

fn resolved_schema<'a>(schema: &'a Value, root: &'a Value) -> &'a Value {
    if let Some(r) = schema["$ref"].as_str() {
        return root.pointer(&r[1..]).unwrap_or(schema);
    }
    if let Some(any) = schema["anyOf"].as_array()
        && let Some(s) = any.iter().find(|s| s["type"] != "null")
    {
        return resolved_schema(s, root);
    }
    schema
}
fn property_object(
    ui: &mut egui::Ui,
    value: &mut Value,
    schema: &Value,
    root: &Value,
    depth: usize,
) {
    let schema = resolved_schema(schema, root);
    let Some(properties) = schema["properties"].as_object() else {
        return;
    };
    for (key, s) in properties {
        if matches!(key.as_str(), "type" | "children" | "id") {
            continue;
        }
        if key == "font" {
            ui.label("Font: Roboto");
            if value
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|font| font != "Roboto")
            {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "Using Roboto; the requested font reference is preserved.",
                );
            }
            continue;
        }
        ui.push_id(key, |ui| {
            let required = schema["required"]
                .as_array()
                .is_some_and(|a| a.contains(&json!(key)));
            let existing = value.get(key).cloned();
            let mut enabled = existing.is_some();
            let mut reset = false;
            ui.horizontal(|ui| {
                if !required {
                    ui.checkbox(&mut enabled, "");
                }
                ui.label(key.replace('_', " "))
                    .on_hover_text(s["description"].as_str().unwrap_or(""));
                if !required && existing.is_some() {
                    reset = ui.small_button("Reset").clicked();
                }
            });
            if reset || !enabled {
                value.as_object_mut().unwrap().remove(key);
                return;
            }
            let mut edited = existing.unwrap_or_else(|| schema_default(s, root));
            edit_value(ui, &mut edited, s, root, depth);
            value[key] = edited;
        });
    }
}
fn schema_default(schema: &Value, root: &Value) -> Value {
    let schema = resolved_schema(schema, root);
    if let Some(default) = schema.get("default") {
        return default.clone();
    }
    if let Some(values) = schema["enum"].as_array() {
        return values[0].clone();
    }
    let t = schema["type"].as_str().or_else(|| {
        schema["type"]
            .as_array()?
            .iter()
            .find_map(|t| t.as_str().filter(|s| *s != "null"))
    });
    match t {
        Some("boolean") => json!(true),
        Some("number") => json!(1.0),
        Some("integer") => json!(1),
        Some("object") => json!({}),
        Some("array") => {
            if let Some(items) = schema["prefixItems"].as_array() {
                Value::Array(items.iter().map(|s| schema_default(s, root)).collect())
            } else {
                Value::Array(
                    (0..schema["minItems"].as_u64().unwrap_or(0))
                        .map(|_| schema_default(&schema["items"], root))
                        .collect(),
                )
            }
        }
        _ => json!(
            if schema["pattern"].as_str().is_some_and(|p| p.contains('#')) {
                "#ffffff"
            } else {
                ""
            }
        ),
    }
}
fn edit_value(ui: &mut egui::Ui, value: &mut Value, schema: &Value, root: &Value, depth: usize) {
    if depth > 8 {
        return;
    }
    let schema = resolved_schema(schema, root);
    if let Some(values) = schema["enum"].as_array() {
        egui::ComboBox::from_id_salt("enum")
            .selected_text(
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            )
            .show_ui(ui, |ui| {
                for option in values {
                    ui.selectable_value(
                        value,
                        option.clone(),
                        option
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| option.to_string()),
                    );
                }
            });
        return;
    }
    match value {
        Value::Bool(v) => {
            ui.checkbox(v, "Enabled");
        }
        Value::Number(v) => {
            let mut n = v.as_f64().unwrap_or(0.0);
            if ui.add(egui::DragValue::new(&mut n).speed(0.1)).changed()
                && let Some(v) = serde_json::Number::from_f64(n)
            {
                *value = if schema["type"] == "integer"
                    || schema["type"]
                        .as_array()
                        .is_some_and(|a| a.contains(&json!("integer")))
                {
                    json!(n.round() as i64)
                } else {
                    Value::Number(v)
                };
            }
        }
        Value::String(v) => {
            if v.starts_with('#')
                && let Some(color) = actionlay_layout::color::Color::parse_hex(v)
            {
                let mut rgba = [color.r, color.g, color.b, color.a];
                if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
                    *v = format!(
                        "#{:02x}{:02x}{:02x}{:02x}",
                        rgba[0], rgba[1], rgba[2], rgba[3]
                    );
                }
            }
            ui.text_edit_singleline(v);
        }
        Value::Object(_) => {
            ui.indent("object", |ui| {
                property_object(ui, value, schema, root, depth + 1);
            });
            if ui.small_button("Reset group").clicked() {
                *value = json!({});
            }
        }
        Value::Array(values) => {
            let mut remove = None;
            for (i, v) in values.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    let s = schema["prefixItems"].get(i).unwrap_or(&schema["items"]);
                    edit_value(ui, v, s, root, depth + 1);
                    if !(schema["minItems"].as_u64().is_some()
                        && schema["minItems"] == schema["maxItems"])
                        && ui.small_button("Remove").clicked()
                    {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                values.remove(i);
            }
            if !(schema["minItems"].as_u64().is_some() && schema["minItems"] == schema["maxItems"])
                && ui.small_button("Add").clicked()
            {
                values.push(schema_default(&schema["items"], root));
            }
        }
        _ => {}
    }
}
fn load_background(ctx: &egui::Context, path: &Path) -> Result<egui::TextureHandle, String> {
    if path.metadata().map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
        return Err("Background image exceeds 32 MB".into());
    }
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|e| e.to_string())?
        .thumbnail(2048, 2048)
        .to_rgba8();
    Ok(ctx.load_texture(
        "editor-background",
        egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Editor {
        Editor::new(Editor::blank(), None, None, true)
    }

    #[test]
    fn all_palette_widgets_are_valid_and_render_without_video() {
        let mut e = session();
        let root = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        for kind in Widget::TYPES {
            e.add(kind, [100.0, 100.0], root);
        }
        e.draft.to_json().unwrap();
        let mut pixmap = Pixmap::new(960, 540).unwrap();
        e.renderer
            .render_editor_into(&e.draft, &e.demo, 30.0, &mut pixmap);
        assert_eq!(e.renderer.hit_boxes().len(), Widget::TYPES.len());
        assert!(pixmap.data().iter().any(|v| *v > 0));
        let context = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            e.ui(ui, None, None, None, 0.0);
        });
        assert!(!output.shapes.is_empty());
        assert!(!e.video_background);
        output.textures_delta.clear();
        drop(e);
        context.tex_manager().write().take_delta().clear();
    }

    #[test]
    fn saving_a_new_layout_turns_it_into_an_existing_document() {
        let mut e = session();
        assert!(e.dirty());
        assert!(e.path.is_none());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("my.ovl.json");
        e.save_to(&path).unwrap();
        assert!(!e.dirty());
        assert_eq!(e.path.as_deref(), Some(path.as_path()));
        e.add(
            "compass",
            [200.0, 100.0],
            Rect::new(0.0, 0.0, 1920.0, 1080.0),
        );
        assert!(e.dirty());
        e.save_to(&path).unwrap();
        assert!(!e.dirty());
        assert_eq!(Layout::load(&path).unwrap().layout, e.draft);
    }

    #[test]
    fn failed_or_invalid_save_preserves_destination_and_saved_baseline() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("saved.ovl.json");
        let mut e = session();
        e.save_to(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        e.add("text", [30.0, 30.0], Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert!(
            e.save_to(&directory.path().join("missing/layout.ovl.json"))
                .is_err()
        );
        assert!(e.dirty());
        assert_eq!(e.path.as_deref(), Some(path.as_path()));
        e.invalid_properties = true;
        assert!(e.save_to(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(e.dirty());
        e.invalid_properties = false;
        if let Node::Known(Widget::Text(t)) = &mut e.draft.nodes[0] {
            t.style.size = Some(-1.0);
        }
        assert!(e.save_to(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn duplicate_group_removes_recursive_ids_and_history_restores_saved_state() {
        let layout=Layout::from_json(r#"{"version":1,"nodes":[{"type":"frame","id":"frame","size":[400,300],"future_key":4,"children":[{"type":"text","id":"title","text":"Hi"}]}]}"#).unwrap().layout;
        let mut e = Editor::new(layout.clone(), None, None, false);
        e.selection = Some(vec![0]);
        e.duplicate();
        assert_eq!(e.draft.nodes.len(), 2);
        assert!(e.dirty());
        assert!(e.draft.nodes[1].id().is_none());
        let Node::Known(w) = &e.draft.nodes[1] else {
            panic!()
        };
        assert!(w.children()[0].id().is_none());
        e.draft.to_json().unwrap();
        e.undo();
        assert_eq!(e.draft, layout);
        assert!(!e.dirty());
        e.redo();
        assert_eq!(e.draft.nodes.len(), 2);
        let out = serde_json::to_value(&e.draft.nodes[1]).unwrap();
        assert_eq!(out["future_key"], 4);
    }

    #[test]
    fn relative_position_survives_window_scaling_and_preserves_nested_anchor() {
        let mut node = template("frame");
        let mut v = serde_json::to_value(&node).unwrap();
        v["anchor"] = json!("bottom-right");
        node = serde_json::from_value(v).unwrap();
        let parent = Rect::new(100.0, 200.0, 800.0, 600.0);
        let desired = Rect::new(380.0, 420.0, 400.0, 240.0);
        set_relative_position(&mut node, parent, desired);
        let Node::Known(w) = &node else { panic!() };
        let c = w.common();
        assert_eq!(
            geom::place(
                parent,
                c.anchor.unwrap(),
                c.offset_in(parent),
                [400.0, 240.0]
            ),
            desired
        );
        let bigger = Rect::new(100.0, 200.0, 1600.0, 1200.0);
        let placed = geom::place(
            bigger,
            c.anchor.unwrap(),
            c.offset_in(bigger),
            [400.0, 240.0],
        );
        assert!((placed.x - 1060.0).abs() < 0.01);
        assert!((placed.y - 880.0).abs() < 0.01);
    }

    #[test]
    fn schema_defaults_make_fixed_pairs_and_integer_fields_editable() {
        let schema = actionlay_layout::json_schema();
        let pairs = &schema["$defs"]["FrameNode"]["properties"]["size"];
        assert_eq!(schema_default(pairs, &schema).as_array().unwrap().len(), 2);
        assert_eq!(
            schema_default(&json!({"type":["integer","null"]}), &schema),
            json!(1)
        );
        let color = schema_default(&schema["$defs"]["ColorRef"], &schema);
        assert!(serde_json::from_value::<actionlay_layout::color::ColorRef>(color).is_ok());
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;

    #[test]
    fn drag_tracks_the_whole_gesture_and_release_does_not_revert_it() {
        let mut editor = Editor::new(Editor::blank(), None, None, true);
        let root = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        editor.add("frame", [100.0, 100.0], root);
        let mut image = Pixmap::new(960, 540).unwrap();
        editor
            .renderer
            .render_editor_into(&editor.draft, &editor.demo, 30.0, &mut image);
        editor.undo.clear();
        let context = egui::Context::default();
        let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 540.0));
        let mut pass = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(canvas),
                events,
                ..Default::default()
            };
            let mut output = context.run_ui(input, |ui| {
                let response = ui.allocate_rect(canvas, egui::Sense::click_and_drag());
                editor.canvas_input(ui, &response, canvas, root, 0.5);
            });
            output.textures_delta.clear();
        };
        pass(Vec::new());
        pass(vec![egui::Event::PointerMoved(egui::pos2(100.0, 100.0))]);
        pass(vec![egui::Event::PointerButton {
            pos: egui::pos2(100.0, 100.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        pass(vec![egui::Event::PointerMoved(egui::pos2(125.0, 125.0))]);
        pass(vec![egui::Event::PointerMoved(egui::pos2(150.0, 150.0))]);
        pass(vec![egui::Event::PointerButton {
            pos: egui::pos2(150.0, 150.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        let Node::Known(w) = &editor.draft.nodes[0] else {
            panic!()
        };
        let offset = w.common().offset_in(root);
        assert!((offset[0] - 200.0).abs() < 0.01, "{offset:?}");
        assert!((offset[1] - 200.0).abs() < 0.01, "{offset:?}");
        assert_eq!(
            editor.undo.len(),
            1,
            "one gesture should make one undo action"
        );
        editor.undo();
        let Node::Known(w) = &editor.draft.nodes[0] else {
            panic!()
        };
        assert_eq!(w.common().offset_in(root), [100.0, 100.0]);
        context.tex_manager().write().take_delta().clear();
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    #[test]
    fn proportional_resize_keeps_the_top_left_of_right_anchored_widgets() {
        let parent = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        for kind in ["map", "compass", "icon", "text"] {
            let mut node = template(kind);
            let mut value = serde_json::to_value(&node).unwrap();
            value["anchor"] = json!("bottom-right");
            if kind == "text" {
                value["size"] = json!(40);
            }
            node = serde_json::from_value(value).unwrap();
            let original = if kind == "map" {
                Rect::new(1500.0, 750.0, 300.0, 240.0)
            } else if kind == "text" {
                Rect::new(1500.0, 750.0, 100.0, 40.0)
            } else {
                Rect::new(1500.0, 750.0, 100.0, 100.0)
            };
            resize_node(&mut node, original, egui::vec2(50.0, 0.0), parent);
            let Node::Known(w) = &node else { panic!() };
            let value = serde_json::to_value(&node).unwrap();
            let size = match kind {
                "map" => [
                    value["size"][0].as_f64().unwrap() as f32,
                    value["size"][1].as_f64().unwrap() as f32,
                ],
                "compass" => [value["diameter"].as_f64().unwrap() as f32; 2],
                "text" => [original.w * 1.5, original.h * 1.5],
                _ => [value["size"].as_f64().unwrap() as f32; 2],
            };
            let placed = geom::place(
                parent,
                w.common().anchor.unwrap(),
                w.common().offset_in(parent),
                size,
            );
            assert!((placed.x - original.x).abs() < 0.01, "{kind}: {placed:?}");
            assert!((placed.y - original.y).abs() < 0.01, "{kind}: {placed:?}");
            assert!((placed.w / placed.h - original.w / original.h).abs() < 0.01);
        }
    }
}
