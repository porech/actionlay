//! Visual layout drafts. Player state changes only after a successful save.
use crate::video_view::{VideoView, fit_rect};
use actionlay_layout::geom::{self, Anchor, Aspect, Rect};
use actionlay_layout::{Layout, Node, Widget};
use actionlay_render::{Renderer, tiny_skia::Pixmap};
use actionlay_telemetry::{Metric, Telemetry};
use eframe::egui;
use serde_json::{Value, json};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;

type NodePath = Vec<usize>;

#[cfg(test)]
#[path = "editor/m4_tests.rs"]
mod m4_tests;

#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Save,
    SaveAs,
    Export,
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
    moving: Vec<actionlay_render::HitBox>,
}

pub struct Editor {
    pub draft: Layout,
    saved: Layout,
    pub path: Option<PathBuf>,
    pub include_fonts: bool,
    is_new: bool,
    selection: Option<NodePath>,
    additional_selection: Vec<NodePath>,
    root: Rect,
    properties_buffer: Option<(NodePath, Value)>,
    invalid_properties: bool,
    property_error: Option<String>,
    undo: Vec<Layout>,
    redo: Vec<Layout>,
    clipboard: Vec<Node>,
    drag: Option<Drag>,
    pub video_background: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub map_progress: Option<crate::telemetry_load::RouteProgress>,
    pub dimensions: [u32; 2],
    snap: bool,
    automatic_anchor: bool,
    renderer: Renderer,
    offline_maps: actionlay_maps::TileStore,
    maps: actionlay_maps::TileStore,
    demo: Telemetry,
    texture: Option<egui::TextureHandle>,
    last_preview: Option<String>,
    background: Option<egui::TextureHandle>,
    schema: Value,
    font_key: Option<String>,
    font_warnings: Vec<String>,
    font_families: Vec<String>,
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
            include_fonts: true,
            is_new,
            selection: None,
            additional_selection: Vec::new(),
            root: Rect::new(0.0, 0.0, 1920.0, 1080.0),
            properties_buffer: None,
            invalid_properties: false,
            property_error: None,
            undo: Vec::new(),
            redo: Vec::new(),
            clipboard: Vec::new(),
            drag: None,
            video_background: video_size.is_some(),
            dimensions: video_size.unwrap_or([1920, 1080]),
            snap: true,
            automatic_anchor: false,
            #[cfg(not(target_arch = "wasm32"))]
            map_progress: None,
            renderer: Renderer::new(),
            offline_maps: actionlay_maps::TileStore::offline(),
            maps: actionlay_maps::TileStore::offline(),
            demo: Telemetry::preview(),
            texture: None,
            last_preview: None,
            background: None,
            schema: actionlay_layout::json_schema(),
            font_key: None,
            font_warnings: Vec::new(),
            font_families: Vec::new(),
            error: None,
        }
    }

    pub fn blank() -> Layout {
        Layout {
            loaded_assets: Default::default(),
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

    pub fn invalid_parameters(&self) -> bool {
        self.invalid_properties
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

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        if self.invalid_properties {
            return Err("Fix invalid widget parameters before saving".into());
        }
        // Validate first, then atomically replace the destination. Neither the
        // saved baseline nor the destination changes on validation/write errors.
        let text = self.draft.to_json().map_err(|e| e.to_string())?;
        if actionlay_layout::package::is_package(path) {
            actionlay_layout::package::save(&self.draft, path).map_err(|e| e.to_string())?;
        } else {
            if !self.draft.loaded_assets.is_empty() {
                return Err("Save this layout as .actionlay-layout to preserve its assets".into());
            }
            let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))
                .map_err(|e| e.to_string())?;
            use std::io::Write;
            file.write_all((text + "\n").as_bytes())
                .and_then(|_| file.as_file().sync_all())
                .map_err(|e| e.to_string())?;
            file.persist(path).map_err(|e| e.to_string())?;
        }
        self.saved = self.draft.clone();
        self.path = Some(path.to_path_buf());
        self.is_new = false;
        self.error = None;
        Ok(())
    }

    /// Browser saves acknowledge the draft only after persistence succeeds.
    #[cfg(target_arch = "wasm32")]
    pub fn acknowledge_save(&mut self) {
        self.saved = self.draft.clone();
        self.is_new = false;
        self.error = None;
    }

    #[cfg(target_arch = "wasm32")]
    pub fn attach_asset(&mut self, name: String, bytes: Vec<u8>) -> Result<(), String> {
        let before = self.draft.clone();
        actionlay_layout::package::attach(&mut self.draft, name, bytes)
            .map_err(|e| e.to_string())?;
        self.commit(before);
        Ok(())
    }

    fn undo(&mut self) {
        if let Some(layout) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.draft, layout));
            self.selection = None;
            self.additional_selection.clear();
            self.properties_buffer = None;
            self.invalid_properties = false;
            self.property_error = None;
        }
    }
    fn redo(&mut self) {
        if let Some(layout) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.draft, layout));
            self.selection = None;
            self.additional_selection.clear();
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
        self.additional_selection.clear();
        self.commit(before);
    }

    fn duplicate(&mut self) {
        let copies = self.selected_copies();
        self.insert_copies(copies);
    }

    fn selected_copies(&self) -> Vec<Node> {
        let parent_path = self
            .selection
            .as_ref()
            .map(|path| path[..path.len() - 1].to_vec())
            .unwrap_or_default();
        let parent = container_box(
            &self.draft.nodes,
            &parent_path,
            self.renderer.hit_boxes(),
            self.root,
        );
        self.selected_paths()
            .iter()
            .filter_map(|path| {
                let mut node = node_at(&self.draft.nodes, path)?.clone();
                if path[..path.len() - 1] != parent_path
                    && let Some(parent) = parent
                    && let Some(hit) = self
                        .renderer
                        .hit_boxes()
                        .iter()
                        .find(|hit| &hit.path == path)
                {
                    let rect = placement_rect(&node, hit);
                    set_relative_position(&mut node, parent, rect);
                }
                Some(node)
            })
            .collect()
    }
    fn delete(&mut self) {
        self.properties_buffer = None;
        self.invalid_properties = false;
        self.property_error = None;
        let paths = self.selected_paths();
        let before = self.draft.clone();
        remove_paths(&mut self.draft.nodes, &paths);
        self.selection = None;
        self.additional_selection.clear();
        self.commit(before);
    }
    fn reorder(&mut self, forward: bool) {
        if self.selected_paths().len() > 1 {
            return;
        }
        if let Some(path) = &mut self.selection {
            self.additional_selection.clear();
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

    fn selected_paths(&self) -> Vec<NodePath> {
        normalized_paths(
            self.selection
                .iter()
                .cloned()
                .chain(self.additional_selection.iter().cloned())
                .collect(),
        )
    }

    fn insert_copies(&mut self, copies: Vec<Node>) {
        if copies.is_empty() {
            return;
        }
        let before = self.draft.clone();
        let parent = self
            .selection
            .as_ref()
            .map(|p| p[..p.len() - 1].to_vec())
            .unwrap_or_default();
        let mut selected = Vec::new();
        if let Some(nodes) = children_at_mut(&mut self.draft.nodes, &parent) {
            for mut node in copies {
                strip_ids(&mut node);
                let mut value = serde_json::to_value(&node).unwrap();
                let offset = value.get("offset").cloned().unwrap_or(json!([0, 0]));
                value["offset"] = json!([
                    offset[0].as_f64().unwrap_or(0.0) + 24.0,
                    offset[1].as_f64().unwrap_or(0.0) + 24.0
                ]);
                node = serde_json::from_value(value).unwrap();
                selected.push(parent.iter().copied().chain([nodes.len()]).collect());
                nodes.push(node);
            }
        }
        self.selection = selected.last().cloned();
        self.additional_selection = selected;
        self.commit(before);
    }

    /// Move selected roots into a container while preserving their current boxes.
    fn reparent(&mut self, target: NodePath) -> Result<(), String> {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return Ok(());
        }
        if paths.iter().any(|path| target.starts_with(path)) {
            return Err("A widget cannot be moved into itself or its descendants".into());
        }
        let parent = container_box(
            &self.draft.nodes,
            &target,
            self.renderer.hit_boxes(),
            self.root,
        )
        .ok_or("Select a group or frame as the destination")?;
        let mut moved = Vec::new();
        for path in &paths {
            let mut node = node_at(&self.draft.nodes, path)
                .ok_or("Selection no longer exists")?
                .clone();
            let hit = self
                .renderer
                .hit_boxes()
                .iter()
                .find(|hit| &hit.path == path)
                .ok_or("The selected widget has no visible preview geometry")?;
            let rect = placement_rect(&node, hit);
            set_relative_position(&mut node, parent, rect);
            moved.push(node);
        }
        let before = self.draft.clone();
        let mut draft = before.clone();
        remove_paths(&mut draft.nodes, &paths);
        let target = remap_path(&target, &paths);
        let nodes =
            children_at_mut(&mut draft.nodes, &target).ok_or("Destination no longer exists")?;
        let selected: Vec<NodePath> = (nodes.len()..nodes.len() + moved.len())
            .map(|index| target.iter().copied().chain([index]).collect())
            .collect();
        nodes.extend(moved);
        draft.to_json().map_err(|e| e.to_string())?;
        self.draft = draft;
        self.selection = selected.last().cloned();
        self.additional_selection = selected;
        self.properties_buffer = None;
        self.commit(before);
        Ok(())
    }

    fn group_selection(&mut self) -> Result<(), String> {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return Ok(());
        }
        let boxes = self.renderer.hit_boxes();
        let rects: Vec<_> = paths
            .iter()
            .filter_map(|path| {
                boxes
                    .iter()
                    .find(|hit| &hit.path == path)
                    .map(|hit| hit.rect)
            })
            .collect();
        if rects.len() != paths.len() {
            return Err("Selected widgets must be visible in the preview".into());
        }
        let bounds = union_rects(&rects).unwrap();
        let parent_path = paths[0][..paths[0].len() - 1].to_vec();
        let parent_path = if paths.iter().all(|p| p[..p.len() - 1] == parent_path) {
            parent_path
        } else {
            Vec::new()
        };
        let parent = container_box(&self.draft.nodes, &parent_path, boxes, self.root)
            .ok_or("Parent geometry unavailable")?;
        let mut children = Vec::new();
        for path in &paths {
            let mut node = node_at(&self.draft.nodes, path).unwrap().clone();
            let hit = boxes.iter().find(|hit| &hit.path == path).unwrap();
            set_relative_position(
                &mut node,
                bounds,
                placement_rect(node_at(&self.draft.nodes, path).unwrap(), hit),
            );
            let old_opacity = parent_opacity(&self.draft.nodes, &path[..path.len() - 1]);
            let new_opacity = parent_opacity(&self.draft.nodes, &parent_path);
            if old_opacity != new_opacity && new_opacity > 0.0 {
                let mut value = serde_json::to_value(&node).unwrap();
                value["opacity"] = json!(
                    value["opacity"].as_f64().unwrap_or(1.0) * (old_opacity / new_opacity) as f64
                );
                node = serde_json::from_value(value).unwrap();
            }
            children.push(node);
        }
        let mut group: Node = serde_json::from_value(json!({"type":"group", "name":"Group", "size":[bounds.w.max(1.0),bounds.h.max(1.0)], "children":children})).unwrap();
        set_relative_position(&mut group, parent, bounds);
        let before = self.draft.clone();
        remove_paths(&mut self.draft.nodes, &paths);
        let parent_path = remap_path(&parent_path, &paths);
        let nodes = children_at_mut(&mut self.draft.nodes, &parent_path).unwrap();
        self.selection = Some(parent_path.into_iter().chain([nodes.len()]).collect());
        nodes.push(group);
        self.additional_selection.clear();
        self.properties_buffer = None;
        self.commit(before);
        Ok(())
    }

    fn ungroup(&mut self) -> Result<(), String> {
        let Some(path) = self.selection.clone() else {
            return Ok(());
        };
        let Some(Node::Known(Widget::Group(group))) = node_at(&self.draft.nodes, &path) else {
            return Err("Select a group to ungroup".into());
        };
        // Preserve opacity/visibility rather than silently changing the result.
        let common = group.common.clone();
        if !group.extra.is_empty() {
            return Err(
                "This group has fields from another version; ungrouping would discard them".into(),
            );
        }
        let parent = self
            .renderer
            .hit_boxes()
            .iter()
            .find(|hit| hit.path == path)
            .ok_or("Group geometry unavailable")?
            .parent;
        let mut children = group.children.clone();
        for (index, child) in children.iter_mut().enumerate() {
            let child_path: Vec<_> = path.iter().copied().chain([index]).collect();
            let hit = self
                .renderer
                .hit_boxes()
                .iter()
                .find(|hit| hit.path == child_path)
                .ok_or("All group children must be visible to ungroup")?;
            let rect = placement_rect(child, hit);
            set_relative_position(child, parent, rect);
            let mut value = serde_json::to_value(&*child).unwrap();
            if let Some(opacity) = common.opacity {
                value["opacity"] = json!(opacity * value["opacity"].as_f64().unwrap_or(1.0) as f32);
            }
            if common.visible == Some(false) {
                value["visible"] = json!(false);
            }
            *child = serde_json::from_value(value).unwrap();
        }
        let before = self.draft.clone();
        let nodes = children_at_mut(&mut self.draft.nodes, &path[..path.len() - 1]).unwrap();
        let index = *path.last().unwrap();
        let selected: Vec<NodePath> = (index..index + children.len())
            .map(|i| path[..path.len() - 1].iter().copied().chain([i]).collect())
            .collect();
        nodes.splice(index..=index, children);
        self.selection = selected.last().cloned();
        self.additional_selection = selected;
        self.properties_buffer = None;
        self.commit(before);
        Ok(())
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
                ui.strong(crate::i18n::ui_text(ui, "Edit layout"));
                ui.label(crate::i18n::ui_text(
                    ui,
                    format!(
                        "{}{}",
                        self.draft.name.as_deref().unwrap_or("Untitled"),
                        if self.dirty() { " *" } else { "" }
                    ),
                ));
                if ui.button(crate::i18n::ui_text(ui, "New layout…")).clicked() {
                    action = Some(Action::New);
                }
                if ui.button(crate::i18n::ui_text(ui, "Save")).clicked() {
                    action = Some(Action::Save);
                }
                if ui.button(crate::i18n::ui_text(ui, "Save as…")).clicked() {
                    action = Some(Action::SaveAs);
                }
                if ui
                    .button(crate::i18n::ui_text(ui, "Export package…"))
                    .clicked()
                {
                    action = Some(Action::Export);
                }
                ui.checkbox(
                    &mut self.include_fonts,
                    crate::i18n::ui_text(ui, "Include fonts"),
                );
                if ui.button(crate::i18n::ui_text(ui, "Exit editor")).clicked() {
                    action = Some(Action::Exit);
                }
                if ui
                    .add_enabled(
                        !self.undo.is_empty(),
                        egui::Button::new(crate::i18n::text("Undo")),
                    )
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(
                        !self.redo.is_empty(),
                        egui::Button::new(crate::i18n::text("Redo")),
                    )
                    .clicked()
                {
                    self.redo();
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(crate::i18n::ui_text(ui, "Background"));
                ui.add_enabled_ui(video_size.is_some(), |ui| {
                    ui.selectable_value(
                        &mut self.video_background,
                        true,
                        crate::i18n::ui_text(ui, "Open video"),
                    );
                });
                ui.selectable_value(
                    &mut self.video_background,
                    false,
                    crate::i18n::text("Static image"),
                );
                if !self.video_background {
                    egui::ComboBox::from_id_salt("preview-aspect")
                        .selected_text(crate::i18n::ui_text(
                            ui,
                            format!("{} × {}", self.dimensions[0], self.dimensions[1]),
                        ))
                        .show_ui(ui, |ui| {
                            for (name, size) in [
                                ("16:9 · Full HD", [1920, 1080]),
                                ("16:9 · 4K", [3840, 2160]),
                                ("4:3", [1440, 1080]),
                                ("9:16", [1080, 1920]),
                                ("1:1", [1080, 1080]),
                            ] {
                                ui.selectable_value(
                                    &mut self.dimensions,
                                    size,
                                    crate::i18n::ui_text(ui, name),
                                );
                            }
                        });
                    for n in &mut self.dimensions {
                        ui.add(egui::DragValue::new(n).range(64..=16384));
                    }
                }
                ui.checkbox(&mut self.snap, crate::i18n::ui_text(ui, "Snap to guides"));
                ui.checkbox(
                    &mut self.automatic_anchor,
                    crate::i18n::ui_text(ui, "Update anchor on drop"),
                );
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
                    self.clipboard = self.selected_copies();
                }
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::V) {
                    self.insert_copies(self.clipboard.clone());
                }
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::A) {
                    self.additional_selection =
                        (0..self.draft.nodes.len()).map(|i| vec![i]).collect();
                    self.selection = self.additional_selection.last().cloned();
                }
            });
        }
        egui::Panel::left("editor-palette")
            .default_size(190.0)
            .resizable(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading(crate::i18n::ui_text(ui, "Widgets"));
                    ui.small(crate::i18n::ui_text(
                        ui,
                        "Drag onto the preview, or click to add.",
                    ));
                    for kind in Widget::TYPES {
                        ui.horizontal(|ui| {
                            ui.dnd_drag_source(
                                egui::Id::new(("palette", kind)),
                                kind.to_owned(),
                                |ui| {
                                    ui.label(crate::i18n::ui_text(
                                        ui,
                                        crate::i18n::widget_label(kind),
                                    ));
                                },
                            );
                            if ui.small_button(crate::i18n::ui_text(ui, "+")).clicked() {
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
                    ui.heading(crate::i18n::ui_text(ui, "Layers"));
                    ui.small(crate::i18n::ui_text(
                        ui,
                        "Ctrl/⌘ or Shift-click to select several widgets.",
                    ));
                    layer_tree(
                        ui,
                        &self.draft.nodes,
                        &mut Vec::new(),
                        &mut self.selection,
                        &mut self.additional_selection,
                        warnings_telemetry,
                    );
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(crate::i18n::ui_text(ui, "Duplicate")).clicked() {
                            self.duplicate();
                        }
                        if ui.button(crate::i18n::ui_text(ui, "Delete")).clicked() {
                            self.delete();
                        }
                        if ui
                            .add_enabled(
                                self.selected_paths().len() == 1,
                                egui::Button::new(crate::i18n::text("Forward")),
                            )
                            .clicked()
                        {
                            self.reorder(true);
                        }
                        if ui
                            .add_enabled(
                                self.selected_paths().len() == 1,
                                egui::Button::new(crate::i18n::text("Backward")),
                            )
                            .clicked()
                        {
                            self.reorder(false);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(crate::i18n::ui_text(ui, "Group")).clicked() {
                            self.error = self.group_selection().err();
                        }
                        if ui.button(crate::i18n::ui_text(ui, "Ungroup")).clicked() {
                            self.error = self.ungroup().err();
                        }
                    });
                    let mut destination = None;
                    egui::ComboBox::from_id_salt("move-to-group")
                        .selected_text(crate::i18n::ui_text(ui, "Move selection to…"))
                        .show_ui(ui, |ui| {
                            if ui.button(crate::i18n::ui_text(ui, "Layout root")).clicked() {
                                destination = Some(Vec::new());
                                ui.close();
                            }
                            for hit in self.renderer.hit_boxes() {
                                if let Some(Node::Known(widget)) =
                                    node_at(&self.draft.nodes, &hit.path)
                                    && matches!(widget, Widget::Group(_) | Widget::Frame(_))
                                    && !self
                                        .selected_paths()
                                        .iter()
                                        .any(|path| hit.path.starts_with(path))
                                    && ui
                                        .button(crate::i18n::ui_text(
                                            ui,
                                            format!(
                                                "{} ({:?})",
                                                widget.common().name.as_deref().unwrap_or("Group"),
                                                hit.path
                                            ),
                                        ))
                                        .clicked()
                                {
                                    destination = Some(hit.path.clone());
                                    ui.close();
                                }
                            }
                        });
                    if let Some(target) = destination {
                        self.error = self.reparent(target).err();
                    }
                });
            });
        egui::Panel::right("editor-properties")
            .default_size(290.0)
            .resizable(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading(crate::i18n::ui_text(ui, "Properties"));
                    self.properties(ui, warnings_telemetry);
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
            if !self.video_background {
                ui.horizontal(|ui| {
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui
                        .button(crate::i18n::text("Load background image…"))
                        .clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                crate::i18n::native_text("Images"),
                                &["png", "jpg", "jpeg", "webp", "bmp"],
                            )
                            .pick_file()
                    {
                        match load_background(&ctx, &path) {
                            Ok(t) => self.background = Some(t),
                            Err(e) => self.error = Some(e),
                        }
                    }
                    if self.background.is_some()
                        && ui.button(crate::i18n::text("Clear image")).clicked()
                    {
                        self.background = None;
                    }
                    ui.small(crate::i18n::ui_text(
                        ui,
                        "Demonstration data · map downloads disabled",
                    ));
                });
            }
            for error in [&self.error, &self.property_error].into_iter().flatten() {
                ui.colored_label(egui::Color32::LIGHT_RED, crate::i18n::ui_text(ui, error));
            }
            let size = if self.video_background {
                video_size.unwrap_or(self.dimensions)
            } else {
                self.dimensions
            };
            let canvas = fit_rect(ui.available_rect_before_wrap(), size[0], size[1]);
            if canvas.width() < 8.0 || canvas.height() < 8.0 {
                ui.label(crate::i18n::ui_text(
                    ui,
                    "Enlarge the window or narrow the side panels to show the preview.",
                ));
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
            self.root = root;
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
            #[cfg(not(target_arch = "wasm32"))]
            if self.video_background
                && let Some(progress) = self.map_progress
            {
                crate::map_loading::show(
                    ui,
                    &self.draft,
                    canvas,
                    mode,
                    telemetry,
                    time,
                    Some(progress),
                );
            }
            for hit in self.renderer.hit_boxes() {
                if let Some(node) = node_at(&self.draft.nodes, &hit.path) {
                    let warnings = metric_warnings(node, warnings_telemetry);
                    if !warnings.is_empty() {
                        let pos = canvas.min
                            + egui::vec2((hit.rect.x + hit.rect.w) * scale, hit.rect.y * scale);
                        let badge = egui::Rect::from_center_size(pos, egui::vec2(20.0, 20.0));
                        ui.painter()
                            .rect_filled(badge, 3.0, egui::Color32::from_rgb(100, 75, 0));
                        ui.painter().text(
                            pos,
                            egui::Align2::CENTER_CENTER,
                            "!",
                            egui::FontId::proportional(15.0),
                            egui::Color32::YELLOW,
                        );
                        ui.interact(
                            badge,
                            egui::Id::new(("data-warning", &hit.path)),
                            egui::Sense::hover(),
                        )
                        .on_hover_text(crate::i18n::ui_text(ui, warnings.join("\n")));
                    }
                }
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
        for path in self.selected_paths() {
            if let Some(hit) = boxes.iter().find(|hit| hit.path == path) {
                ui.painter().rect_stroke(
                    screen(hit.rect),
                    0.0,
                    egui::Stroke::new(1.5, egui::Color32::LIGHT_BLUE),
                    egui::StrokeKind::Outside,
                );
            }
        }
        let mut resize_response = None;
        if let Some(hit) = selected.filter(|_| self.selected_paths().len() <= 1) {
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
                    crate::i18n::text("Selected widget extends outside the frame"),
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
                let additive = ctx.input(|i| i.modifiers.command || i.modifiers.shift);
                let already_selected = self.selected_paths().contains(&hit.path);
                if response.clicked() || !already_selected {
                    select_path(
                        &mut self.selection,
                        &mut self.additional_selection,
                        hit.path.clone(),
                        additive,
                    );
                }
                if resizing || response.drag_started() {
                    let paths = self.selected_paths();
                    let moving = boxes
                        .iter()
                        .filter(|hit| paths.contains(&hit.path))
                        .cloned()
                        .collect();
                    self.drag = Some(Drag {
                        start: ctx
                            .input(|i| i.pointer.press_origin())
                            .unwrap_or_else(|| ctx.pointer_interact_pos().unwrap_or_default()),
                        path: hit.path,
                        before: self.draft.clone(),
                        rect: hit.rect,
                        parent: hit.parent,
                        resize: resizing,
                        moving,
                    });
                }
            } else if response.clicked() {
                self.selection = None;
                self.additional_selection.clear();
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
                    let mut rect =
                        union_rects(&drag.moving.iter().map(|hit| hit.rect).collect::<Vec<_>>())
                            .unwrap_or(drag.rect);
                    let original = rect;
                    rect.x += delta.x / scale;
                    rect.y += delta.y / scale;
                    if self.snap && !ctx.input(|i| i.modifiers.alt) {
                        let targets: Vec<_> = boxes
                            .iter()
                            .filter(|hit| {
                                !drag.moving.iter().any(|moving| {
                                    hit.path.starts_with(&moving.path)
                                        || moving.path.starts_with(&hit.path)
                                })
                            })
                            .map(|hit| hit.rect)
                            .chain([root])
                            .collect();
                        let guides = snap_to_rects(&mut rect, &targets, 8.0 / scale);
                        if let Some(x) = guides[0] {
                            let x = canvas.min.x + x * scale;
                            ui.painter().line_segment(
                                [egui::pos2(x, canvas.top()), egui::pos2(x, canvas.bottom())],
                                egui::Stroke::new(1.0, egui::Color32::YELLOW),
                            );
                        }
                        if let Some(y) = guides[1] {
                            let y = canvas.min.y + y * scale;
                            ui.painter().line_segment(
                                [egui::pos2(canvas.left(), y), egui::pos2(canvas.right(), y)],
                                egui::Stroke::new(1.0, egui::Color32::YELLOW),
                            );
                        }
                    }
                    let point = anchor_point(node, drag.parent);
                    let anchor = canvas.min + egui::vec2(point[0] * scale, point[1] * scale);
                    ui.painter().line_segment(
                        [anchor, screen(rect).center()],
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 125, 160)),
                    );
                    for moving in &drag.moving {
                        if let Some(node) = node_at_mut(&mut layout.nodes, &moving.path) {
                            let mut moved = placement_rect(node, moving);
                            moved.x += rect.x - original.x;
                            moved.y += rect.y - original.y;
                            if self.automatic_anchor
                                && moving.parent.w > 0.0
                                && moving.parent.h > 0.0
                            {
                                let mut value = serde_json::to_value(&*node).unwrap();
                                value["anchor"] =
                                    serde_json::to_value(nearest_anchor(moved, moving.parent))
                                        .unwrap();
                                *node = serde_json::from_value(value).unwrap();
                            }
                            set_relative_position(node, moving.parent, moved);
                        }
                    }
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
        #[cfg(not(target_arch = "wasm32"))]
        if ui
            .button(crate::i18n::text("Attach font or image…"))
            .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter(
                    crate::i18n::native_text("Layout assets"),
                    &["ttf", "otf", "ttc", "png", "jpg", "jpeg", "webp", "svg"],
                )
                .pick_file()
        {
            let result = (|| -> Result<(), String> {
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                if actionlay_render::fonts::is_font(&path.to_string_lossy())
                    && !actionlay_render::fonts::valid_font(&bytes)
                {
                    return Err("This file is not a usable font".into());
                }
                let stem = path.file_name().unwrap_or_default().to_string_lossy();
                let mut name = format!("assets/{stem}");
                let mut index = 2;
                while self.draft.loaded_assets.contains_key(&name) {
                    name = format!("assets/{index}-{stem}");
                    index += 1;
                }
                actionlay_layout::package::attach(&mut self.draft, name, bytes)
                    .map_err(|e| e.to_string())
            })();
            self.error = result.err();
        }
        ui.small(crate::i18n::ui_text(ui, "Custom fonts are scoped to this layout. Set a font family in its theme or widget properties."));
        let font_key = actionlay_render::fonts::configuration_key(&self.draft);
        if self.font_key.as_ref() != Some(&font_key) {
            self.font_warnings = actionlay_render::fonts::warnings(&self.draft);
            self.font_families = actionlay_render::fonts::families(&self.draft);
            self.font_key = Some(font_key);
        }
        let mut font = None;
        egui::ComboBox::from_id_salt("editor-font-family")
            .selected_text(crate::i18n::ui_text(ui, "Choose font family…"))
            .show_ui(ui, |ui| {
                for family in &self.font_families {
                    if ui.button(crate::i18n::ui_text(ui, family)).clicked() {
                        font = Some(family.clone());
                        ui.close();
                    }
                }
            });
        if let Some(font) = font {
            if let Some(path) = &self.selection
                && let Some(node) = node_at_mut(&mut self.draft.nodes, path)
            {
                let mut value = serde_json::to_value(&*node).unwrap();
                if matches!(
                    node.type_name(),
                    "text" | "metric" | "metric_unit" | "datetime"
                ) {
                    value["font"] = json!(font);
                    *node = serde_json::from_value(value).unwrap();
                    self.properties_buffer = None;
                } else {
                    self.draft.theme.get_or_insert_default().font = Some(font);
                }
            } else {
                self.draft.theme.get_or_insert_default().font = Some(font);
            }
        }
        for warning in &self.font_warnings {
            ui.colored_label(egui::Color32::YELLOW, crate::i18n::ui_text(ui, warning));
        }
        if let Some(path) = self.selection.clone()
            && let Some(node) = node_at(&self.draft.nodes, &path)
        {
            if self.selected_paths().len() > 1 {
                ui.small(crate::i18n::ui_text(
                    ui,
                    format!(
                        "{} widgets selected; properties apply to this widget",
                        self.selected_paths().len()
                    ),
                ));
            }
            ui.strong(crate::i18n::ui_text(
                ui,
                crate::i18n::widget_label(node.type_name()),
            ));
            for warning in metric_warnings(node, telemetry) {
                ui.colored_label(egui::Color32::YELLOW, crate::i18n::ui_text(ui, warning));
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
                if ui
                    .button(crate::i18n::ui_text(ui, "Reset widget parameters"))
                    .clicked()
                {
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
                if node.type_name() == "map" {
                    map_properties(ui, &mut value);
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
                ui.weak(crate::i18n::ui_text(
                    ui,
                    "This widget is preserved but is not supported by this version.",
                ));
            }
        } else {
            ui.label(crate::i18n::ui_text(ui, "Layout name"));
            let name = self.draft.name.get_or_insert_default();
            ui.text_edit_singleline(name);
            ui.small(crate::i18n::ui_text(
                ui,
                "Select a widget in the preview or in Layers to edit it.",
            ));
            ui.label(crate::i18n::ui_text(ui, "Design proportions"));
            let aspect = self.draft.design_aspect.get_or_insert(Aspect::WIDESCREEN);
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut aspect.w).range(1.0..=16384.0));
                ui.label(crate::i18n::ui_text(ui, ":"));
                ui.add(egui::DragValue::new(&mut aspect.h).range(1.0..=16384.0));
            });
            let mut value = serde_json::to_value(&self.draft).unwrap();
            let schema = json!({"properties":{"theme":{"anyOf":[{"$ref":"#/$defs/Theme"},{"type":"null"}]},"units":{"anyOf":[{"$ref":"#/$defs/Units"},{"type":"null"}]}}});
            property_object(ui, &mut value, &schema, &self.schema, 0);
            if let Ok(layout) = serde_json::from_value::<Layout>(value)
                && layout.to_json().is_ok()
            {
                let assets = self.draft.loaded_assets.clone();
                self.draft = layout;
                self.draft.loaded_assets = assets;
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
    additional: &mut Vec<NodePath>,
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
        let name = if name == node.type_name() {
            crate::i18n::native_text(&crate::i18n::widget_label(name)).to_owned()
        } else {
            name.to_owned()
        };
        let label = format!("{}{}", if warnings.is_empty() { "" } else { "⚠ " }, name);
        if ui
            .selectable_label(
                selected.as_ref() == Some(path) || additional.contains(path),
                crate::i18n::user_text(ui, label),
            )
            .on_hover_text(crate::i18n::ui_text(ui, warnings.join("\n")))
            .clicked()
        {
            let additive = ui.input(|i| i.modifiers.command || i.modifiers.shift);
            select_path(selected, additional, path.clone(), additive);
        }
        if let Node::Known(w) = node
            && !w.children().is_empty()
        {
            ui.indent(egui::Id::new(path.clone()), |ui| {
                layer_tree(ui, w.children(), path, selected, additional, telemetry)
            });
        }
        path.pop();
    }
}

fn normalized_paths(mut paths: Vec<NodePath>) -> Vec<NodePath> {
    paths.sort();
    paths.dedup();
    let mut roots: Vec<NodePath> = Vec::new();
    for path in paths {
        if !path.is_empty() && !roots.iter().any(|root| path.starts_with(root)) {
            roots.push(path);
        }
    }
    roots
}

fn select_path(
    primary: &mut Option<NodePath>,
    additional: &mut Vec<NodePath>,
    path: NodePath,
    additive: bool,
) {
    if !additive {
        additional.clear();
        *primary = Some(path);
        return;
    }
    if let Some(primary) = primary.as_ref()
        && !additional.contains(primary)
    {
        additional.push(primary.clone());
    }
    if let Some(index) = additional.iter().position(|selected| selected == &path) {
        additional.remove(index);
        *primary = additional.last().cloned();
    } else {
        if let Some(ancestor) = additional
            .iter()
            .find(|selected| path.starts_with(selected))
            .cloned()
        {
            *primary = Some(ancestor);
            return;
        }
        additional.retain(|selected| !selected.starts_with(&path));
        additional.push(path.clone());
        *primary = Some(path);
    }
}

fn remove_paths(nodes: &mut Vec<Node>, paths: &[NodePath]) {
    for path in paths.iter().rev() {
        if let Some(children) = children_at_mut(nodes, &path[..path.len() - 1]) {
            children.remove(*path.last().unwrap());
        }
    }
}

fn remap_path(path: &[usize], removed: &[NodePath]) -> NodePath {
    path.iter()
        .enumerate()
        .map(|(depth, index)| {
            index
                - removed
                    .iter()
                    .filter(|removed| {
                        removed.len() == depth + 1
                            && removed[..depth] == path[..depth]
                            && removed[depth] < *index
                    })
                    .count()
        })
        .collect()
}

fn container_box(
    nodes: &[Node],
    path: &[usize],
    boxes: &[actionlay_render::HitBox],
    root: Rect,
) -> Option<Rect> {
    if path.is_empty() {
        return Some(root);
    }
    let Node::Known(widget) = node_at(nodes, path)? else {
        return None;
    };
    let size = match widget {
        Widget::Group(group) => group.size.unwrap_or([0.0; 2]),
        Widget::Frame(frame) => frame.size,
        _ => return None,
    };
    let hit = boxes.iter().find(|hit| hit.path == path)?;
    let common = widget.common();
    Some(geom::place(
        hit.parent,
        common.anchor.unwrap_or_default(),
        common.offset_in(hit.parent),
        size,
    ))
}

fn placement_rect(node: &Node, hit: &actionlay_render::HitBox) -> Rect {
    if let Node::Known(Widget::Group(group)) = node {
        let common = &group.common;
        geom::place(
            hit.parent,
            common.anchor.unwrap_or_default(),
            common.offset_in(hit.parent),
            group.size.unwrap_or([0.0; 2]),
        )
    } else {
        hit.rect
    }
}

fn nearest_anchor(rect: Rect, parent: Rect) -> Anchor {
    let x = ((rect.x + rect.w / 2.0 - parent.x) / parent.w.max(1.0) * 2.0)
        .round()
        .clamp(0.0, 2.0) as usize;
    let y = ((rect.y + rect.h / 2.0 - parent.y) / parent.h.max(1.0) * 2.0)
        .round()
        .clamp(0.0, 2.0) as usize;
    Anchor::ALL[y * 3 + x]
}

fn parent_opacity(nodes: &[Node], path: &[usize]) -> f32 {
    (1..=path.len())
        .filter_map(|length| node_at(nodes, &path[..length]))
        .filter_map(|node| {
            if let Node::Known(widget) = node {
                Some(widget.common().opacity.unwrap_or(1.0))
            } else {
                None
            }
        })
        .product()
}

fn union_rects(rects: &[Rect]) -> Option<Rect> {
    let first = rects.first()?;
    let mut left = first.x;
    let mut top = first.y;
    let mut right = first.x + first.w;
    let mut bottom = first.y + first.h;
    for rect in &rects[1..] {
        left = left.min(rect.x);
        top = top.min(rect.y);
        right = right.max(rect.x + rect.w);
        bottom = bottom.max(rect.y + rect.h);
    }
    Some(Rect::new(left, top, right - left, bottom - top))
}

/// Align the nearest pair of edges or centres on each axis. Return guide positions.
fn snap_to_rects(rect: &mut Rect, targets: &[Rect], tolerance: f32) -> [Option<f32>; 2] {
    let mut guides = [None; 2];
    for (axis, pos, length) in [(0, &mut rect.x, rect.w), (1, &mut rect.y, rect.h)] {
        let mut best = tolerance;
        let mut correction = 0.0;
        for target in targets {
            let (origin, total) = if axis == 0 {
                (target.x, target.w)
            } else {
                (target.y, target.h)
            };
            for point in [origin, origin + total / 2.0, origin + total] {
                for offset in [0.0, length / 2.0, length] {
                    let delta = point - (*pos + offset);
                    if delta.abs() < best {
                        best = delta.abs();
                        correction = delta;
                        guides[axis] = Some(point);
                    }
                }
            }
        }
        *pos += correction;
    }
    guides
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
fn map_properties(ui: &mut egui::Ui, value: &mut Value) {
    ui.group(|ui| {
        for (key,label,default,options) in [
            ("orientation","Orientation","north_up",&[("north_up","North up"),("course_up","Direction of travel up")][..]),
            ("route_mode","Route","none",&[("none","No route"),("past","Completed route only"),("full","Entire route from the start")][..]),
        ] {
            let mut selected=value[key].as_str().unwrap_or(default).to_owned();
            let old=selected.clone();
            egui::ComboBox::from_id_salt(key).selected_text(crate::i18n::ui_text(ui, options.iter().find(|o|o.0==selected).map_or(selected.as_str(),|o|o.1)))
                .show_ui(ui,|ui| { for &(id,text) in options { ui.selectable_value(&mut selected,id.to_owned(),crate::i18n::ui_text(ui, text)); } });
            ui.small(crate::i18n::ui_text(ui, label));
            if selected != old { value[key]=json!(selected); }
            if value.get(key).is_some() && ui.small_button(crate::i18n::ui_text(ui, format!("Reset {label}"))).clicked() { value.as_object_mut().unwrap().remove(key); }
        }
        let mut zoom_mode = value["zoom_mode"].as_str().unwrap_or("fixed").to_owned();
        let before = zoom_mode.clone();
        ui.label(crate::i18n::text("Zoom mode"));
        egui::ComboBox::from_id_salt("map-zoom-mode").selected_text(crate::i18n::text(if zoom_mode == "route" { "Fit entire route" } else { "Fixed zoom" })).show_ui(ui, |ui| {
            ui.selectable_value(&mut zoom_mode, "fixed".to_owned(), crate::i18n::text("Fixed zoom"));
            ui.selectable_value(&mut zoom_mode, "route".to_owned(), crate::i18n::text("Fit entire route"));
        });
        if zoom_mode != before { value["zoom_mode"] = json!(zoom_mode); }
        if zoom_mode == "route" {
            let mut coverage = value["route_coverage"].as_f64().unwrap_or(0.8);
            if ui.add(egui::Slider::new(&mut coverage, 0.1..=1.0).text(crate::i18n::text("Route size")).custom_formatter(|n, _| format!("{:.0}%", n * 100.0))).changed() { value["route_coverage"] = json!(coverage); }
            ui.small(crate::i18n::ui_text(ui, "Uses the complete route, even when only the completed section is drawn. Fixed zoom is used while loading."));
        }
        let mut split=value["split_route"].as_bool().unwrap_or(true);
        if ui.checkbox(&mut split,crate::i18n::ui_text(ui, "Different color for the upcoming route")).changed() { value["split_route"]=json!(split); }

    });
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
        if value["type"] == "map"
            && matches!(
                key.as_str(),
                "orientation" | "route_mode" | "split_route" | "zoom_mode" | "route_coverage"
            )
        {
            continue;
        }
        if matches!(key.as_str(), "type" | "children" | "id") {
            continue;
        }
        if key == "units" && value.get("metric").is_none() {
            let selected = value["units"].as_str().unwrap_or("default").to_owned();
            let mut chosen = selected.clone();
            ui.label(crate::i18n::ui_text(ui, crate::i18n::property_label(key)));
            egui::ComboBox::from_id_salt((depth, key))
                .selected_text(crate::i18n::ui_text(ui, crate::i18n::enum_label(&selected)))
                .show_ui(ui, |ui| {
                    for id in ["default", "metric", "imperial"] {
                        ui.selectable_value(
                            &mut chosen,
                            id.to_owned(),
                            crate::i18n::ui_text(ui, crate::i18n::enum_label(id)),
                        );
                    }
                });
            if chosen != selected {
                value[key] = json!(chosen);
            }
            continue;
        }
        if key == "units" && s.to_string().contains("string") && value.get("metric").is_some() {
            let selected = value["units"].as_str().unwrap_or("default").to_owned();
            let mut chosen = selected.clone();
            ui.label(crate::i18n::ui_text(ui, crate::i18n::property_label(key)));
            egui::ComboBox::from_id_salt((depth, key))
                .selected_text(if selected == "default" {
                    crate::i18n::text("Default")
                } else {
                    &selected
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut chosen,
                        "default".to_owned(),
                        crate::i18n::text("Default"),
                    );
                    if let Some(metric) = value["metric"]
                        .as_str()
                        .and_then(actionlay_telemetry::metric::Metric::from_id)
                    {
                        for unit in actionlay_telemetry::units::units_for(metric.quantity()) {
                            ui.selectable_value(
                                &mut chosen,
                                unit.id().to_owned(),
                                if actionlay_telemetry::units::symbol(*unit).is_empty() {
                                    crate::i18n::text("None")
                                } else {
                                    actionlay_telemetry::units::symbol(*unit)
                                },
                            );
                        }
                    }
                });
            if chosen != selected {
                value[key] = json!(chosen);
            }
            continue;
        }
        ui.push_id(key, |ui| {
            let required = key == "stale_secs" || schema["required"]
                .as_array()
                .is_some_and(|a| a.contains(&json!(key)));
            let existing = value.get(key).cloned();
            let mut enabled = existing.is_some() || key == "stale_secs";
            let mut reset = false;
            ui.horizontal(|ui| {
                if !required {
                    ui.checkbox(&mut enabled, crate::i18n::ui_text(ui, ""));
                }
                let label = if value["type"] == "map" {
                    match key.as_str() {
                        "route" => "Completed route color".into(),
                        "route_future" => "Upcoming route color".into(),
                        "route_width" => "Route line width".into(),
                        "marker" => "Position dot color".into(),
                        "marker_radius" => "Position dot radius".into(),
                        "show_marker" => "Show position dot".into(),
                        _ => crate::i18n::property_label(key),
                    }
                } else {
                    crate::i18n::property_label(key)
                };
                ui.label(crate::i18n::ui_text(ui, label))
                    .on_hover_text(crate::i18n::ui_text(ui, if key == "stale_secs" { "Retain the last valid value during missing data for this many video seconds. Buffering does not count as missing data." } else { s["description"].as_str().unwrap_or("") }));
                if !required && existing.is_some() {
                    reset = ui.small_button(crate::i18n::ui_text(ui, "Reset")).clicked();
                }
            });
            if reset || !enabled {
                value.as_object_mut().unwrap().remove(key);
                return;
            }
            let was_present = existing.is_some();
            let mut edited = existing.unwrap_or_else(|| if key == "stale_secs" { json!(3.0) } else { schema_default(s, root) });
            let before = edited.clone();
            edit_value(ui, &mut edited, s, root, depth);
            if key != "stale_secs" || was_present || edited != before { value[key] = edited; }
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
            .selected_text(crate::i18n::ui_text(
                ui,
                value
                    .as_str()
                    .map(crate::i18n::enum_label)
                    .unwrap_or_else(|| value.to_string()),
            ))
            .show_ui(ui, |ui| {
                for option in values {
                    ui.selectable_value(
                        value,
                        option.clone(),
                        crate::i18n::ui_text(
                            ui,
                            option
                                .as_str()
                                .map(crate::i18n::enum_label)
                                .unwrap_or_else(|| option.to_string()),
                        ),
                    );
                }
            });
        return;
    }
    match value {
        Value::Bool(v) => {
            ui.checkbox(v, crate::i18n::ui_text(ui, "Enabled"));
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
            if ui
                .small_button(crate::i18n::ui_text(ui, "Reset group"))
                .clicked()
            {
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
                        && ui
                            .small_button(crate::i18n::ui_text(ui, "Remove"))
                            .clicked()
                    {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                values.remove(i);
            }
            if !(schema["minItems"].as_u64().is_some() && schema["minItems"] == schema["maxItems"])
                && ui.small_button(crate::i18n::ui_text(ui, "Add")).clicked()
            {
                values.push(schema_default(&schema["items"], root));
            }
        }
        _ => {}
    }
}
#[cfg(not(target_arch = "wasm32"))]
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
