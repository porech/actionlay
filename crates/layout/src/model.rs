//! Layout tree (spec §4.1, §4.3). Optional parameters are `None` = inherit (see `style`).
use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::color::ColorRef;
use crate::geom::Anchor;
use crate::style::TextStyleOpt;

/// Keys this version does not know, kept verbatim and written back on save (spec §6.4).
pub type Extra = BTreeMap<String, serde_json::Value>;

/// Parameters every node has.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Common {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Default: top-left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    /// [x, y] from the anchor in layout units (positive = right/down). Default [0, 0].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<[f32; 2]>,
    /// 0..=1, multiplies down the tree. Default 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    /// Default true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
}

/// What a data widget shows when its metric is absent (spec §4.4.1).
///
/// `Hide` hides the widget only when the video never has the metric
/// (`Snapshot::is_available` is false), not during a momentary gap in the data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WhenAbsent {
    /// Designed empty state (dimmed "—").
    #[default]
    Show,
    /// The widget is not drawn.
    Hide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DateZone {
    /// Time zone of the machine (M2; per-video zones come later).
    #[default]
    Local,
    Utc,
}

/// Default unit system of a layout (spec §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    #[default]
    Metric,
    Imperial,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BorderOpt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    #[serde(flatten)]
    pub extra: Extra,
}

// Like the theme groups (style.rs): a border with every field unset is not serialized,
// so resetting its last field leaves no empty object behind.
fn border_unset(b: &Option<BorderOpt>) -> bool {
    b.as_ref()
        .is_none_or(|b| b.color.is_none() && b.width.is_none() && b.extra.is_empty())
}

/// Container. With `size`, children anchor inside it; without, they are placed
/// relative to the group's origin (the original's `composite`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GroupNode {
    #[serde(flatten)]
    pub common: Common,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Group with a size, a background panel, a border and rounded corners.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FrameNode {
    #[serde(flatten)]
    pub common: Common,
    pub size: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<ColorRef>,
    #[serde(default, skip_serializing_if = "border_unset")]
    pub border: Option<BorderOpt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextNode {
    #[serde(flatten)]
    pub common: Common,
    pub text: String,
    #[serde(flatten)]
    pub style: TextStyleOpt,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MetricNode {
    #[serde(flatten)]
    pub common: Common,
    /// Metric id, e.g. "speed" (spec §4.4).
    pub metric: String,
    /// Unit id overriding the layout's unit system, e.g. "kmh".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// Format mini-language, default "{value:.0}".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_absent: Option<WhenAbsent>,
    /// How long a stale value stays visible (dimmed) before the empty state. Default 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_secs: Option<f32>,
    #[serde(flatten)]
    pub style: TextStyleOpt,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MetricUnitNode {
    #[serde(flatten)]
    pub common: Common,
    pub metric: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_absent: Option<WhenAbsent>,
    #[serde(flatten)]
    pub style: TextStyleOpt,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DatetimeNode {
    #[serde(flatten)]
    pub common: Common,
    /// strftime, default "%H:%M:%S".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<DateZone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_absent: Option<WhenAbsent>,
    #[serde(flatten)]
    pub style: TextStyleOpt,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IconNode {
    #[serde(flatten)]
    pub common: Common,
    /// Name from the embedded icon set (see actionlay-render).
    pub icon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GpsLockIconNode {
    #[serde(flatten)]
    pub common: Common,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// Colour with a 3D lock (default: accent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Node types this version understands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Widget {
    Group(GroupNode),
    Frame(FrameNode),
    Text(TextNode),
    Metric(MetricNode),
    MetricUnit(MetricUnitNode),
    Datetime(DatetimeNode),
    Icon(IconNode),
    GpsLockIcon(GpsLockIconNode),
}

impl Widget {
    pub const TYPES: [&'static str; 8] = [
        "group",
        "frame",
        "text",
        "metric",
        "metric_unit",
        "datetime",
        "icon",
        "gps_lock_icon",
    ];

    pub fn common(&self) -> &Common {
        match self {
            Widget::Group(n) => &n.common,
            Widget::Frame(n) => &n.common,
            Widget::Text(n) => &n.common,
            Widget::Metric(n) => &n.common,
            Widget::MetricUnit(n) => &n.common,
            Widget::Datetime(n) => &n.common,
            Widget::Icon(n) => &n.common,
            Widget::GpsLockIcon(n) => &n.common,
        }
    }

    pub fn extra(&self) -> &Extra {
        match self {
            Widget::Group(n) => &n.extra,
            Widget::Frame(n) => &n.extra,
            Widget::Text(n) => &n.extra,
            Widget::Metric(n) => &n.extra,
            Widget::MetricUnit(n) => &n.extra,
            Widget::Datetime(n) => &n.extra,
            Widget::Icon(n) => &n.extra,
            Widget::GpsLockIcon(n) => &n.extra,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Widget::Group(_) => "group",
            Widget::Frame(_) => "frame",
            Widget::Text(_) => "text",
            Widget::Metric(_) => "metric",
            Widget::MetricUnit(_) => "metric_unit",
            Widget::Datetime(_) => "datetime",
            Widget::Icon(_) => "icon",
            Widget::GpsLockIcon(_) => "gps_lock_icon",
        }
    }

    pub fn children(&self) -> &[Node] {
        match self {
            Widget::Group(n) => &n.children,
            Widget::Frame(n) => &n.children,
            _ => &[],
        }
    }
}

/// A node of the layout tree: a known widget, or a node of a type this version does
/// not know (written by a newer ActionLay), kept verbatim and not drawn.
// Boxing would make every `Node::Known(w)` pattern in the render crate clumsier for no gain.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Known(Widget),
    Unknown(serde_json::Value),
}

impl Node {
    pub fn type_name(&self) -> &str {
        match self {
            Node::Known(w) => w.type_name(),
            Node::Unknown(v) => v.get("type").and_then(|t| t.as_str()).unwrap_or(""),
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Node::Known(w) => w.common().id.as_deref(),
            Node::Unknown(v) => v.get("id").and_then(|t| t.as_str()),
        }
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Node::Known(w) => w.serialize(s),
            Node::Unknown(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        let ty = value
            .get("type")
            .and_then(|t| t.as_str())
            .ok_or_else(|| D::Error::custom(NO_TYPE))?;
        if !Widget::TYPES.contains(&ty) {
            return Ok(Node::Unknown(value));
        }
        match Widget::deserialize(&value) {
            Ok(w) => Ok(Node::Known(w)),
            Err(e) => {
                let (path, message) = node_error(&value).unwrap_or_else(|| {
                    // not reproducible on its own: report what serde said
                    (
                        String::new(),
                        format!("{ty} node{}: {e}", quoted_id(&value)),
                    )
                });
                Err(D::Error::custom(at_path(&path, &message)))
            }
        }
    }
}

const NO_TYPE: &str = "node without a string `type`";

fn quoted_id(value: &serde_json::Value) -> String {
    value
        .get("id")
        .and_then(|t| t.as_str())
        .map(|i| format!(" `{i}`"))
        .unwrap_or_default()
}

/// `path: message`, or the message alone when the path is empty.
pub(crate) fn at_path(path: &str, message: &str) -> String {
    if path.is_empty() {
        message.to_string()
    } else {
        format!("{path}: {message}")
    }
}

fn join(parent: &str, child: &str) -> String {
    if child.is_empty() {
        parent.to_string()
    } else if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}.{child}")
    }
}

/// Why the node `value` does not deserialize, and where relative to it:
/// `("children[1].anchor", "text node `t`: unknown variant `middle`, …")`. The path is
/// empty when the field cannot be told for sure. `None` if the node is fine. Nodes of
/// unknown types are never searched (they are kept verbatim, so they cannot fail).
pub(crate) fn node_error(value: &serde_json::Value) -> Option<(String, String)> {
    let Some(ty) = value.get("type").and_then(|t| t.as_str()) else {
        return Some((String::new(), NO_TYPE.to_string()));
    };
    if !Widget::TYPES.contains(&ty) {
        return None;
    }
    // a failing child is the culprit
    if let Some(children) = value.get("children").and_then(|c| c.as_array()) {
        for (i, child) in children.iter().enumerate() {
            if let Some((path, message)) = node_error(child) {
                return Some((join(&format!("children[{i}]"), &path), message));
            }
        }
    }
    let e = Widget::deserialize(value).err()?;
    let field = bad_value(&e.to_string()).and_then(|bad| unique_key(value, bad, &["children"]));
    Some((
        field.unwrap_or_default(),
        format!("{ty} node{}: {e}", quoted_id(value)),
    ))
}

/// The value serde rejected as unknown (an enum variant, a colour, an aspect), if the
/// message is about one. In other messages the last quoted token may be anything,
/// e.g. the node id of a `metric node `lat`:` prefix.
pub(crate) fn bad_value(message: &str) -> Option<&str> {
    // the bad value is the last `quoted` token before the list of accepted values
    let head = message.split("expected").next().unwrap_or_default();
    let bad = head.rsplit('`').nth(1)?;
    let before = head.rsplit_once(&format!("`{bad}`")).map_or("", |(b, _)| b);
    ["variant ", "colour ", "aspect "]
        .iter()
        .any(|w| before.ends_with(w))
        .then_some(bad)
}

/// Path of the only key of `object` (or of an object nested in it, such as a style
/// group) whose value is the string `wanted`; `None` if there is no such key or more
/// than one. Free-text keys and the keys in `skip` are not searched.
pub(crate) fn unique_key(
    object: &serde_json::Value,
    wanted: &str,
    skip: &[&str],
) -> Option<String> {
    fn walk(
        v: &serde_json::Value,
        wanted: &str,
        skip: &[&str],
        path: &str,
        found: &mut Vec<String>,
    ) {
        let Some(map) = v.as_object() else { return };
        for (k, item) in map {
            if matches!(k.as_str(), "id" | "name" | "text" | "type" | "$schema")
                || skip.contains(&k.as_str())
            {
                continue;
            }
            let p = join(path, k);
            match item {
                serde_json::Value::String(s) if s == wanted => found.push(p),
                serde_json::Value::Object(_) => walk(item, wanted, skip, &p, found),
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    walk(object, wanted, skip, "", &mut found);
    match found.len() {
        1 => found.pop(),
        _ => None,
    }
}

impl JsonSchema for Node {
    fn schema_name() -> Cow<'static, str> {
        "Node".into()
    }

    // The published schema describes the node types of this version; nodes of other
    // types are accepted by the loader (kept, not drawn) but not by the schema.
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        Widget::json_schema(generator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{Color, ColorRef};
    use crate::geom::Anchor;
    use serde_json::json;

    /// Parses, serializes and re-parses a node; returns it with its serialized form.
    fn round_trip(value: serde_json::Value) -> (Node, serde_json::Value) {
        let node: Node = serde_json::from_value(value).unwrap();
        let out = serde_json::to_value(&node).unwrap();
        let again: Node = serde_json::from_value(out.clone()).unwrap();
        assert_eq!(again, node);
        (node, out)
    }

    fn keys(v: &serde_json::Value) -> Vec<&str> {
        let mut k: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        k.sort_unstable();
        k
    }

    #[test]
    fn minimal_metric_node_inherits_everything() {
        let (node, out) = round_trip(json!({"type": "metric", "metric": "speed"}));
        let Node::Known(Widget::Metric(m)) = node else {
            panic!("not a metric: {node:?}")
        };
        assert_eq!(m.common, Common::default());
        assert_eq!(m.style, TextStyleOpt::default());
        assert!(m.units.is_none() && m.format.is_none() && m.extra.is_empty());
        assert_eq!(
            keys(&out),
            ["metric", "type"],
            "defaults are not written back"
        );
    }

    #[test]
    fn spec_example_round_trips() {
        let (node, out) = round_trip(json!({
            "id": "speed-main", "type": "metric", "anchor": "bottom-left", "offset": [16, -120],
            "metric": "speed", "units": "kmh", "format": "{value:.0}", "size": 160, "color": "#ffffff"
        }));
        let Node::Known(Widget::Metric(m)) = node else {
            panic!()
        };
        assert_eq!(m.common.anchor, Some(Anchor::BottomLeft));
        assert_eq!(m.common.offset, Some([16.0, -120.0]));
        assert_eq!(m.style.size, Some(160.0));
        assert_eq!(
            m.style.color,
            Some(ColorRef::Color(Color::rgba(255, 255, 255, 255)))
        );
        assert_eq!(
            keys(&out),
            [
                "anchor", "color", "format", "id", "metric", "offset", "size", "type", "units"
            ]
        );
    }

    #[test]
    fn unknown_fields_and_node_types_survive_round_trip() {
        let input = json!({
            "type": "group", "id": "g", "future_layout_hint": true,
            "children": [
                {"type": "moving_map", "id": "map", "zoom": 15, "size": 256},
                {"type": "metric", "metric": "alt", "glow": {"radius": 4}}
            ]
        });
        let (node, out) = round_trip(input.clone());
        let Node::Known(Widget::Group(g)) = &node else {
            panic!()
        };
        assert_eq!(g.extra.get("future_layout_hint"), Some(&json!(true)));
        assert!(matches!(&g.children[0], Node::Unknown(v) if v["zoom"] == json!(15)));
        assert_eq!(g.children[0].type_name(), "moving_map");
        assert_eq!(g.children[0].id(), Some("map"));
        let Node::Known(Widget::Metric(m)) = &g.children[1] else {
            panic!()
        };
        assert_eq!(
            m.extra.len(),
            1,
            "only the unknown key lands in extra: {:?}",
            m.extra
        );
        assert_eq!(m.extra.get("glow"), Some(&json!({"radius": 4})));
        // unknown parts are written back unchanged
        assert_eq!(out["children"][0], input["children"][0]);
        assert_eq!(out["children"][1]["glow"], json!({"radius": 4}));
        assert_eq!(out["future_layout_hint"], json!(true));
    }

    #[test]
    fn every_widget_type_parses() {
        let nodes = json!([
            {"type": "group", "size": [100, 50], "children": [{"type": "text", "text": "hi"}]},
            {"type": "frame", "size": [300, 100], "radius": 8, "fill": "panel",
             "border": {"color": "accent", "width": 2}},
            {"type": "text", "text": "SPEED", "weight": "medium"},
            {"type": "metric", "metric": "speed", "when_absent": "hide", "stale_secs": 1.5},
            {"type": "metric_unit", "metric": "speed", "units": "mph"},
            {"type": "datetime", "format": "%H:%M", "timezone": "utc"},
            {"type": "icon", "icon": "altitude", "size": 32, "color": "accent"},
            {"type": "gps_lock_icon", "size": 40}
        ]);
        let parsed: Vec<Node> = serde_json::from_value(nodes).unwrap();
        let types: Vec<&str> = parsed.iter().map(Node::type_name).collect();
        assert_eq!(types, Widget::TYPES);
        assert!(
            parsed
                .iter()
                .all(|n| matches!(n, Node::Known(w) if w.extra().is_empty()))
        );
    }

    #[test]
    fn empty_border_group_is_not_serialized() {
        let mut f = FrameNode {
            size: [10.0, 10.0],
            border: Some(BorderOpt {
                width: Some(2.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = serde_json::to_value(&f).unwrap();
        assert_eq!(out["border"], json!({"width": 2.0}));
        f.border.as_mut().unwrap().width = None; // "Reset to default" of the last field
        assert_eq!(
            serde_json::to_value(&f).unwrap(),
            json!({"size": [10.0, 10.0]})
        );
        // a group holding only keys of a newer version is kept
        f.border
            .as_mut()
            .unwrap()
            .extra
            .insert("dash".into(), json!([2, 2]));
        assert_eq!(
            serde_json::to_value(&f).unwrap()["border"],
            json!({"dash": [2, 2]})
        );
    }

    #[test]
    fn invalid_known_node_names_its_type_and_id() {
        let err =
            serde_json::from_value::<Node>(json!({"type": "metric", "id": "spd", "metric": 42}))
                .unwrap_err()
                .to_string();
        assert!(err.contains("metric") && err.contains("spd"), "{err}");
        let err = serde_json::from_value::<Node>(json!({"metric": "speed"}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("type"), "{err}");
    }
}
