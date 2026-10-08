//! Editor-only scaling: layouts keep ordinary, editable widget parameters.
use super::*;
use actionlay_layout::style::{ResolvedTheme, TextKind, TextStyleOpt, defaults};

pub(super) const HINT: &str =
    "Drag to resize the container. Hold Shift to scale it with all its children.";

pub(super) fn resize_section(
    node: &mut Node,
    rect: Rect,
    delta: egui::Vec2,
    parent: Rect,
    theme: &ResolvedTheme,
) {
    if !matches!(node, Node::Known(Widget::Group(_) | Widget::Frame(_)))
        || rect.w <= 0.0
        || rect.h <= 0.0
    {
        resize_node(node, rect, delta, parent);
        return;
    }
    // Project the pointer onto the original diagonal to keep the aspect ratio.
    let factor = ((rect.w * (rect.w + delta.x) + rect.h * (rect.h + delta.y))
        / (rect.w * rect.w + rect.h * rect.h).max(1.0))
    .max(8.0 / rect.w.max(1.0))
    .max(8.0 / rect.h.max(1.0));
    if (factor - 1.0).abs() <= f32::EPSILON {
        return;
    }
    let Node::Known(widget) = &*node else { return };
    let old_size = match widget {
        Widget::Group(g) => g.size.unwrap_or([0.0; 2]),
        Widget::Frame(f) => f.size,
        _ => unreachable!(),
    };
    let common = widget.common();
    let origin = geom::place(
        parent,
        common.anchor.unwrap_or_default(),
        common.offset_in(parent),
        old_size,
    );
    let mut value = serde_json::to_value(&*node).unwrap();
    let new_size = [rect.w * factor, rect.h * factor];
    value["size"] = json!(new_size);
    scale_panel(
        &mut value,
        factor,
        defaults::FRAME_RADIUS,
        defaults::BORDER_WIDTH,
    );
    if let Some(children) = value.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            let Ok(mut child_node) = serde_json::from_value::<Node>(child.clone()) else {
                continue;
            };
            scale_child(
                &mut child_node,
                factor,
                old_size,
                new_size,
                [(origin.x - rect.x) * factor, (origin.y - rect.y) * factor],
                theme,
            );
            *child = serde_json::to_value(child_node).unwrap();
        }
    }
    if let Ok(mut updated) = serde_json::from_value::<Node>(value) {
        set_relative_position(
            &mut updated,
            parent,
            Rect::new(rect.x, rect.y, new_size[0], new_size[1]),
        );
        *node = updated;
    }
}

fn scale_number(value: &mut Value, key: &str, fallback: f32, factor: f32) {
    let old = value[key].as_f64().unwrap_or(fallback as f64);
    value[key] = json!(old * factor as f64);
}

fn scale_panel(value: &mut Value, factor: f32, radius: f32, border: f32) {
    if value["type"] == "group" {
        return;
    }
    scale_number(value, "radius", radius, factor);
    if value["border"].is_null() {
        value["border"] = json!({});
    }
    scale_number(&mut value["border"], "width", border, factor);
}

fn scale_style(value: &mut Value, fallback_size: Option<f32>, factor: f32, theme: &ResolvedTheme) {
    if value.is_null() {
        *value = json!({});
    }
    let opt: TextStyleOpt = serde_json::from_value(value.clone()).unwrap_or_default();
    let style = opt.resolve(TextKind::Text, theme);
    if let Some(size) = opt.size.or(fallback_size) {
        value["size"] = json!(size * factor);
    }
    if value["outline"].is_null() {
        value["outline"] = json!({});
    }
    value["outline"]["width"] = json!(style.outline.width * factor);
    if value["shadow"].is_null() {
        value["shadow"] = json!({});
    }
    value["shadow"]["offset"] = json!(style.shadow.offset.map(|v| v * factor));
}

fn scale_child(
    node: &mut Node,
    factor: f32,
    old_parent: [f32; 2],
    new_parent: [f32; 2],
    translation: [f32; 2],
    theme: &ResolvedTheme,
) {
    // Unknown nodes belong to a newer format; keep their fields verbatim.
    let Node::Known(widget) = &*node else { return };
    let common = widget.common();
    let offset = common.offset.unwrap_or([0.0; 2]);
    let relative = common.offset_relative.unwrap_or([0.0; 2]);
    let (ax, ay) = common.anchor.unwrap_or_default().fractions();
    let anchor = [ax, ay];
    let old_size = match widget {
        Widget::Group(g) => g.size.unwrap_or([0.0; 2]),
        Widget::Frame(f) => f.size,
        _ => [0.0; 2],
    };
    let mut value = serde_json::to_value(&*node).unwrap();
    value["offset"] = json!([0, 1].map(|i| offset[i] * factor
        + translation[i]
        + (relative[i] + anchor[i]) * (old_parent[i] * factor - new_parent[i])));
    match widget {
        Widget::Group(g) => {
            if let Some(size) = g.size {
                value["size"] = json!(size.map(|v| v * factor));
            }
        }
        Widget::Frame(f) => {
            value["size"] = json!(f.size.map(|v| v * factor));
            scale_panel(
                &mut value,
                factor,
                defaults::FRAME_RADIUS,
                defaults::BORDER_WIDTH,
            );
        }
        Widget::Text(_) | Widget::Metric(_) | Widget::MetricUnit(_) | Widget::Datetime(_) => {
            let kind = match widget {
                Widget::Metric(_) => TextKind::Metric,
                Widget::MetricUnit(_) => TextKind::MetricUnit,
                Widget::Datetime(_) => TextKind::Datetime,
                _ => TextKind::Text,
            };
            scale_style(&mut value, Some(kind.defaults().size), factor, theme);
        }
        Widget::Icon(_) | Widget::GpsLockIcon(_) => {
            scale_number(&mut value, "size", defaults::ICON_SIZE, factor)
        }
        Widget::Bar(_) | Widget::ZoneBar(_) => {
            let bar = match widget {
                Widget::Bar(b) => b,
                Widget::ZoneBar(b) => &b.bar,
                _ => unreachable!(),
            };
            value["size"] = json!(bar.size().map(|v| v * factor));
            scale_panel(&mut value, factor, 6.0, 1.0);
            scale_style(
                &mut value["value_style"],
                Some(26.0_f32.min(bar.size()[1] * 0.65)),
                factor,
                theme,
            );
        }
        Widget::Gauge(_) | Widget::Compass(_) => {
            let dial = match widget {
                Widget::Gauge(g) => &g.dial,
                Widget::Compass(c) => &c.dial,
                _ => unreachable!(),
            };
            value["diameter"] = json!(dial.diameter() * factor);
            value["thickness"] = json!(dial.thickness() * factor);
            // These default font sizes already follow the dial diameter.
            scale_style(&mut value["value_style"], None, factor, theme);
            scale_style(&mut value["label_style"], None, factor, theme);
        }
        Widget::Chart(c) | Widget::GradientChart(c) => {
            value["size"] = json!(c.size().map(|v| v * factor));
            scale_number(&mut value, "radius", 12.0, factor);
            scale_number(&mut value, "stroke_width", 2.0, factor);
            scale_style(&mut value["value_style"], Some(18.0), factor, theme);
        }
        Widget::Map(m) => {
            value["size"] = json!(m.size().map(|v| v * factor));
            scale_number(&mut value, "radius", 14.0, factor);
            scale_number(&mut value, "route_width", 3.0, factor);
            scale_number(&mut value, "marker_radius", 5.0, factor);
            // Preserve the map's distinct default sizes for attribution and empty state.
            scale_style(&mut value["label_style"], None, factor, theme);
        }
        Widget::GMeter(g) => {
            value["diameter"] = json!(g.diameter() * factor);
            scale_style(&mut value["value_style"], Some(18.0), factor, theme);
        }
    }
    if let Some(children) = value.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            let Ok(mut child_node) = serde_json::from_value::<Node>(child.clone()) else {
                continue;
            };
            scale_child(
                &mut child_node,
                factor,
                old_size,
                old_size.map(|v| v * factor),
                [0.0; 2],
                theme,
            );
            *child = serde_json::to_value(child_node).unwrap();
        }
    }
    if let Ok(updated) = serde_json::from_value(value) {
        *node = updated;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_scaling_preserves_relative_anchors_defaults_hidden_nodes_and_data_parameters() {
        let mut node: Node = serde_json::from_value(json!({
            "type":"frame","offset":[100,100],"size":[400,300],"radius":12,"border":{"width":2},
            "children":[
                {"type":"group","size":[200,100],"anchor":"bottom-right","offset":[-20,-10],"offset_relative":[-0.1,-0.2],"children":[
                    {"type":"metric","metric":"speed","anchor":"center","offset":[8,6],"visible":false},
                    {"type":"chart","metric":"alt","seconds":90,"min":10,"max":200,"samples":123,"stroke_width":4},
                    {"type":"compass","metric":"heading","smoothing":{"seconds":2},"label_style":{"size":15}},
                    {"type":"future_widget","size":[20,30],"future_number":99}
                ]},
                {"type":"text","text":"Hello","size":32,"font":"Roboto","outline":{"width":4}},
                {"type":"map","zoom":14,"label_style":{"size":13}},
                {"type":"icon","icon":"heart"}
            ]
        })).unwrap();
        let theme: ResolvedTheme = serde_json::from_value::<actionlay_layout::style::Theme>(
            json!({"shadow":{"offset":[2,3]}}),
        )
        .unwrap()
        .resolve();
        resize_section(
            &mut node,
            Rect::new(100.0, 100.0, 400.0, 300.0),
            egui::vec2(400.0, 300.0),
            Rect::new(0.0, 0.0, 1920.0, 1080.0),
            &theme,
        );
        let value = serde_json::to_value(node).unwrap();
        assert_eq!(value["size"], json!([800.0, 600.0]));
        assert_eq!(value["radius"], json!(24.0));
        assert_eq!(value["border"]["width"], json!(4.0));
        let group = &value["children"][0];
        assert_eq!(group["size"], json!([400.0, 200.0]));
        assert_eq!(group["offset"], json!([-40.0, -20.0]));
        assert_eq!(group["offset_relative"], json!([-0.1_f32, -0.2_f32]));
        let metric = &group["children"][0];
        assert_eq!(metric["size"], json!(128.0));
        assert_eq!(metric["visible"], json!(false));
        assert_eq!(metric["shadow"]["offset"], json!([4.0, 6.0]));
        let chart = &group["children"][1];
        assert_eq!(chart["size"], json!([840.0, 320.0]));
        assert_eq!(chart["stroke_width"], json!(8.0));
        assert_eq!(chart["seconds"], json!(90.0));
        assert_eq!(chart["samples"], json!(123));
        assert_eq!(chart["min"], json!(10.0));
        assert_eq!(chart["max"], json!(200.0));
        let compass = &group["children"][2];
        assert_eq!(compass["diameter"], json!(540.0));
        assert_eq!(compass["label_style"]["size"], json!(30.0));
        assert_eq!(compass["smoothing"]["seconds"], json!(2.0));
        assert_eq!(
            group["children"][3],
            json!({"type":"future_widget","size":[20,30],"future_number":99})
        );
        assert_eq!(value["children"][1]["outline"]["width"], json!(8.0));
        assert_eq!(value["children"][2]["zoom"], json!(14));
        assert_eq!(value["children"][2]["label_style"]["size"], json!(26.0));
        assert_eq!(value["children"][3]["size"], json!(80.0));
        assert!(value["children"][3].get("children").is_none());
    }

    #[test]
    fn resizing_a_group_without_size_scales_about_its_visible_bounds() {
        let mut e = Editor::new(Layout::from_json(r#"{"version":1,"nodes":[{"type":"group","id":"group","offset":[400,200],"children":[{"type":"frame","id":"child","anchor":"center","offset":[30,40],"size":[100,60]}]}]}"#).unwrap().layout, None, None, false);
        let mut image = Pixmap::new(960, 540).unwrap();
        e.renderer
            .render_editor_into(&e.draft, &e.demo, 30.0, &mut image);
        let bounds = e
            .renderer
            .hit_boxes()
            .iter()
            .find(|b| b.path == vec![0])
            .unwrap()
            .rect;
        let root = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        resize_section(
            &mut e.draft.nodes[0],
            bounds,
            egui::vec2(bounds.w, bounds.h),
            root,
            &ResolvedTheme::default(),
        );
        e.renderer
            .render_editor_into(&e.draft, &e.demo, 30.0, &mut image);
        let child = e
            .renderer
            .hit_boxes()
            .iter()
            .find(|b| b.path == vec![0, 0])
            .unwrap()
            .rect;
        assert!((child.x - bounds.x).abs() < 0.01);
        assert!((child.y - bounds.y).abs() < 0.01);
        assert!((child.w - bounds.w * 2.0).abs() < 0.01);
        assert!((child.h - bounds.h * 2.0).abs() < 0.01);
        e.draft.to_json().unwrap();
    }
}
