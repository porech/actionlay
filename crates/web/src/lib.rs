//! Browser entry point. Native executables never depend on this crate.
#![cfg(target_arch = "wasm32")]
#![allow(dead_code)] // The shared editor/i18n modules also expose native app helpers.
use actionlay_layout::Layout;
use actionlay_render::{Renderer, tiny_skia::Pixmap};
use actionlay_telemetry::{RawPacket, Telemetry, TelemetryOptions};
use eframe::egui;
use std::{cell::RefCell, io::Cursor, rc::Rc};
use wasm_bindgen::prelude::*;

#[path = "../../app/src/editor.rs"]
mod editor;
#[path = "../../app/src/i18n.rs"]
mod i18n;
mod layouts {
    pub fn scale_mode_for(
        w: u32,
        h: u32,
        layout: &actionlay_layout::Layout,
    ) -> actionlay_layout::geom::ScaleMode {
        actionlay_layout::scale::auto_scale_mode(layout, w, h)
    }
}
mod video_view {
    use eframe::egui;
    pub struct VideoView;
    impl VideoView {
        pub fn show(&mut self, _: &mut egui::Ui, _: egui::Rect, _: Option<egui::Rect>, _: bool) {}
    }
    pub fn fit_rect(available: egui::Rect, w: u32, h: u32) -> egui::Rect {
        let aspect = w as f32 / h as f32;
        let mut size = available.size();
        if size.x / size.y > aspect {
            size.x = size.y * aspect;
        } else {
            size.y = size.x / aspect;
        }
        egui::Rect::from_center_size(available.center(), size)
    }
}
fn error(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}
struct State {
    layout: Layout,
    telemetry: Telemetry,
    packets: Vec<RawPacket>,
    renderer: Renderer,
    pixels: Option<Pixmap>,
    maps: actionlay_maps::TileStore,
    editor: Option<editor::Editor>,
    action: u8,
}
#[wasm_bindgen]
pub struct Core(Rc<RefCell<State>>);
impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}
#[wasm_bindgen]
impl Core {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        let maps = actionlay_maps::TileStore::new(Default::default(), None, || {});
        let mut renderer = Renderer::new();
        renderer.set_maps(maps.clone());
        Self(Rc::new(RefCell::new(State {
            layout: actionlay_layout::default_layout(),
            telemetry: Telemetry::empty(0.0),
            packets: Vec::new(),
            renderer,
            pixels: None,
            maps,
            editor: None,
            action: 0,
        })))
    }
    pub fn presets() -> String {
        serde_json::to_string(
            &actionlay_layout::catalog::PRESETS
                .iter()
                .map(|p| serde_json::json!({"id":p.id,"name":p.name}))
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    pub fn select_preset(&self, id: &str) -> Result<(), JsValue> {
        let layout = actionlay_layout::catalog::find(id)
            .ok_or_else(|| error("Unknown preset"))?
            .layout();
        self.set_layout(layout);
        Ok(())
    }
    pub fn load_layout(&self, bytes: &[u8], name: &str) -> Result<String, JsValue> {
        let loaded = if name.to_ascii_lowercase().ends_with(".actionlay-layout") {
            actionlay_layout::package::load_reader(Cursor::new(bytes)).map_err(error)?
        } else if name.to_ascii_lowercase().ends_with(".xml") {
            {
                let imported = actionlay_layout::import::xml(
                    std::str::from_utf8(bytes).map_err(error)?,
                    name,
                    [1920, 1080],
                )
                .map_err(error)?;
                actionlay_layout::Loaded {
                    layout: imported.layout,
                    warnings: imported.warnings,
                }
            }
        } else {
            Layout::from_json(std::str::from_utf8(bytes).map_err(error)?).map_err(error)?
        };
        if !loaded.layout.loaded_assets.is_empty()
            || actionlay_layout::package::asset_names(&loaded.layout)
                .map_err(error)?
                .is_empty()
        {
            let warnings = loaded
                .warnings
                .iter()
                .map(|i| format!("{}: {}", i.path, i.message))
                .collect::<Vec<_>>()
                .join("\n");
            self.set_layout(loaded.layout);
            Ok(warnings)
        } else {
            Err(error(
                "This JSON references external assets. Import a .actionlay-layout package containing them.",
            ))
        }
    }
    pub fn layout_json(&self) -> Result<String, JsValue> {
        self.validate_editor()?;
        self.current_layout().to_json().map_err(error)
    }
    pub fn package_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.validate_editor()?;
        let mut layout = self.current_layout();
        if self
            .0
            .borrow()
            .editor
            .as_ref()
            .is_some_and(|e| !e.include_fonts)
        {
            actionlay_render::fonts::omit_fonts(&mut layout).map_err(error)?;
        }
        let mut bytes = Cursor::new(Vec::new());
        actionlay_layout::package::write_to(&layout, &mut bytes).map_err(error)?;
        Ok(bytes.into_inner())
    }
    pub fn reset_video(&self, duration: f64) {
        let mut s = self.0.borrow_mut();
        s.packets.clear();
        s.telemetry = Telemetry::empty(duration);
    }
    pub fn add_packet(&self, pts: f64, duration: f64, data: &[u8]) {
        self.0.borrow_mut().packets.push(RawPacket {
            pts,
            duration,
            data: data.to_vec(),
        });
    }
    pub fn update_telemetry(&self) -> Result<(), JsValue> {
        let mut s = self.0.borrow_mut();
        if !s.packets.is_empty() {
            s.packets.sort_by(|a, b| a.pts.total_cmp(&b.pts));
            s.telemetry = Telemetry::from_gpmf_packets_progressive(&s.packets).map_err(error)?;
        }
        Ok(())
    }
    pub fn finish_telemetry(&self, duration: f64) -> Result<usize, JsValue> {
        let mut s = self.0.borrow_mut();
        let options = TelemetryOptions {
            video_duration: Some(duration),
            ..Default::default()
        };
        let mut packets = std::mem::take(&mut s.packets);
        packets.sort_by(|a, b| a.pts.total_cmp(&b.pts));
        let count = packets.len();
        if count > 0 {
            s.telemetry = Telemetry::from_gpmf_packets_with(&packets, &options).map_err(error)?;
        }
        Ok(count)
    }
    /// Premultiplied RGBA from the same renderer used by desktop preview/export.
    pub fn render(&self, time: f64, width: u32, height: u32) -> Result<Vec<u8>, JsValue> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || u64::from(width) * u64::from(height) > 33_554_432
        {
            return Err(error("Unsupported render dimensions"));
        }
        let mut s = self.0.borrow_mut();
        let layout = s.layout.clone();
        let mode = layouts::scale_mode_for(width, height, &layout);
        let State {
            renderer,
            telemetry,
            pixels,
            ..
        } = &mut *s;
        if pixels
            .as_ref()
            .is_none_or(|p| p.width() != width || p.height() != height)
        {
            *pixels = Pixmap::new(width, height);
        }
        let target = pixels
            .as_mut()
            .ok_or_else(|| error("Unable to allocate overlay"))?;
        renderer.set_scale_mode(mode);
        renderer.render_editor_into(&layout, telemetry, time, target);
        Ok(target.data().to_vec())
    }
    /// Same route requirement as desktop: full only when requested by the layout.
    pub fn route_read_until(&self, time: f64, duration: f64) -> f64 {
        use actionlay_layout::model::{MapRoute, Node, Widget};
        fn required(nodes: &[Node], time: f64, duration: f64) -> f64 {
            nodes
                .iter()
                .filter_map(|node| {
                    let Node::Known(w) = node else {
                        return None;
                    };
                    if w.common().visible == Some(false) || w.common().opacity == Some(0.0) {
                        return None;
                    }
                    Some(match w {
                        Widget::Map(m) if m.needs_full_track() => duration,
                        Widget::Map(m) if m.route_mode.unwrap_or_default() == MapRoute::Past => {
                            time
                        }
                        _ => required(w.children(), time, duration),
                    })
                })
                .fold(0.0, f64::max)
        }
        required(&self.0.borrow().layout.nodes, time, duration)
    }
    /// Map loader placement follows the renderer, including nested layout groups.
    pub fn map_regions(&self, width: u32, height: u32) -> String {
        use actionlay_layout::{
            geom::{Aspect, scale_factor},
            model::{MapRoute, Node, Widget},
        };
        let s = self.0.borrow();
        let scale = scale_factor(
            layouts::scale_mode_for(width, height, &s.layout),
            width as f32,
            height as f32,
            s.layout.design_aspect.unwrap_or(Aspect::WIDESCREEN).ratio(),
        );
        let regions: Vec<_> = s
            .renderer
            .hit_boxes()
            .iter()
            .filter_map(|hit| {
                let mut nodes = s.layout.nodes.as_slice();
                let mut widget = None;
                for &index in &hit.path {
                    let Node::Known(w) = nodes.get(index)? else {
                        return None;
                    };
                    widget = Some(w);
                    nodes = w.children();
                }
                let Some(Widget::Map(map)) = widget else {
                    return None;
                };
                if !map.needs_full_track() && map.route_mode.unwrap_or_default() == MapRoute::None {
                    return None;
                }
                Some([
                    hit.rect.x * scale / width as f32,
                    hit.rect.y * scale / height as f32,
                    hit.rect.w * scale / width as f32,
                    hit.rect.h * scale / height as f32,
                ])
            })
            .collect();
        serde_json::to_string(&regions).unwrap()
    }
    pub fn pending_maps(&self) -> usize {
        self.0.borrow().maps.pending_count()
    }
    pub fn configure(&self, units: &str, online_maps: bool, language: &str) {
        use actionlay_layout::model::Units;
        actionlay_render::regional::configure(Some(if units == "imperial" {
            Units::Imperial
        } else {
            Units::Metric
        }));
        i18n::activate(Some(language));
        let s = self.0.borrow();
        let mut settings = s.maps.settings();
        settings.online = online_maps;
        s.maps.configure(settings);
    }
    pub fn start_editing(&self) {
        let mut s = self.0.borrow_mut();
        let mut editor = editor::Editor::new(s.layout.clone(), None, None, false);
        editor.set_maps(s.maps.clone());
        s.editor = Some(editor);
        s.action = 0;
    }
    pub fn new_layout(&self) {
        let mut s = self.0.borrow_mut();
        let mut editor = editor::Editor::new(editor::Editor::blank(), None, None, true);
        editor.set_maps(s.maps.clone());
        s.editor = Some(editor);
        s.action = 0;
    }
    pub fn editor_dirty(&self) -> bool {
        self.0.borrow().editor.as_ref().is_some_and(|e| e.dirty())
    }
    pub fn acknowledge_save(&self) -> Result<(), JsValue> {
        let mut s = self.0.borrow_mut();
        if let Some(editor) = &mut s.editor {
            if editor.invalid_parameters() {
                return Err(error("Fix invalid widget parameters before saving"));
            }
            editor.draft.to_json().map_err(error)?;
            editor.acknowledge_save();
            s.layout = editor.draft.clone();
        }
        Ok(())
    }
    pub fn close_editor(&self) {
        self.0.borrow_mut().editor = None;
    }
    pub fn take_editor_action(&self) -> u8 {
        std::mem::take(&mut self.0.borrow_mut().action)
    }
    pub fn attach_asset(&self, name: &str, data: &[u8]) -> Result<(), JsValue> {
        let mut s = self.0.borrow_mut();
        if actionlay_render::fonts::is_font(name) && !actionlay_render::fonts::valid_font(data) {
            return Err(error("Unusable font"));
        }
        let stem = std::path::Path::new(name)
            .file_name()
            .ok_or_else(|| error("Invalid asset name"))?
            .to_string_lossy();
        let mut key = format!("assets/{stem}");
        let mut n = 2;
        while s
            .editor
            .as_ref()
            .map_or(&s.layout, |e| &e.draft)
            .loaded_assets
            .contains_key(&key)
        {
            key = format!("assets/{n}-{stem}");
            n += 1;
        }
        if let Some(editor) = &mut s.editor {
            editor.attach_asset(key, data.to_vec()).map_err(error)
        } else {
            actionlay_layout::package::attach(&mut s.layout, key, data.to_vec()).map_err(error)
        }
    }
}
impl Core {
    fn validate_editor(&self) -> Result<(), JsValue> {
        if self
            .0
            .borrow()
            .editor
            .as_ref()
            .is_some_and(|e| e.invalid_parameters())
        {
            return Err(error("Fix invalid widget parameters before saving"));
        }
        Ok(())
    }
    fn set_layout(&self, layout: Layout) {
        let mut s = self.0.borrow_mut();
        s.layout = layout;
        s.editor = None;
    }
    fn current_layout(&self) -> Layout {
        let s = self.0.borrow();
        s.editor
            .as_ref()
            .map_or_else(|| s.layout.clone(), |e| e.draft.clone())
    }
}
struct EditorApp(Rc<RefCell<State>>);
impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let mut s = self.0.borrow_mut();
        let Some(editor) = &mut s.editor else {
            return;
        };
        match editor.ui(ui, None, None, None, 0.0) {
            Some(editor::Action::Save | editor::Action::SaveAs | editor::Action::Export) => {
                s.action = 1
            }
            Some(editor::Action::Exit) => s.action = 2,
            Some(editor::Action::New) => s.action = 3,
            None => {}
        }
    }
}
#[wasm_bindgen]
pub async fn start_editor(core: &Core, canvas: web_sys::HtmlCanvasElement) -> Result<(), JsValue> {
    let state = core.0.clone();
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(move |cc| {
                cc.egui_ctx.set_visuals(egui::Visuals::dark());
                i18n::install_fonts(&cc.egui_ctx);
                Ok(Box::new(EditorApp(state)))
            }),
        )
        .await
}
