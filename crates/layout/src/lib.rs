//! ActionLay overlay layouts (`*.ovl.json`): model, defaults, validation, geometry.
//! Pure data: no dependency on other ActionLay crates (spec §2).
pub mod color;
pub mod format;
pub mod geom;
pub mod model;
pub mod style;
pub mod validate;

use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use model::{Extra, Node, Widget};
use validate::{Issue, Severity};

/// File name suffix of layouts.
pub const FILE_SUFFIX: &str = ".ovl.json";

/// The layout shipped with the app, used until the user picks another one.
pub const DEFAULT_LAYOUT_JSON: &str = include_str!("../layouts/default.ovl.json");

/// Layout format version written by this build.
pub const CURRENT_VERSION: u32 = 1;

/// A `*.ovl.json` overlay layout (spec §4.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "ActionLay overlay layout")]
pub struct Layout {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Default 16:9; used by the `fit` scale mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_aspect: Option<geom::Aspect>,
    /// Default unit system; widgets can override it with `units`. Default metric.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<model::Units>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<style::Theme>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A layout that passed validation, with the warnings to show the user.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub layout: Layout,
    pub warnings: Vec<Issue>,
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("cannot read layout: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid layout JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid layout:\n{}", list(.0))]
    Invalid(Vec<Issue>),
}

fn list(issues: &[Issue]) -> String {
    issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

impl Layout {
    /// Parses and validates a layout. Unknown fields, unknown node types and a newer
    /// `version` load with warnings (spec §6.4); see [`validate`] for what is rejected.
    pub fn from_json(text: &str) -> Result<Loaded, LayoutError> {
        let layout: Layout =
            serde_json::from_str(text).map_err(|e| LayoutError::Json(name_the_field(text, e)))?;
        let (errors, warnings): (Vec<Issue>, Vec<Issue>) = validate::validate(&layout)
            .into_iter()
            .partition(|i| i.severity == Severity::Error);
        if errors.is_empty() {
            Ok(Loaded { layout, warnings })
        } else {
            Err(LayoutError::Invalid(errors))
        }
    }

    pub fn load(path: &Path) -> Result<Loaded, LayoutError> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a layout always serializes")
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_json() + "\n")
    }
}

/// serde names a bad value (`unknown variant `middle``, `invalid colour `blue``) but,
/// through the flattened structs of the model, not the key that holds it. Find that key
/// in the document and put its path in front of the message:
/// `nodes[0].anchor: text node `t`: unknown variant `middle`, expected one of …`.
fn name_the_field(text: &str, e: serde_json::Error) -> serde_json::Error {
    if !e.is_data() {
        return e;
    }
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return e;
    };
    let message = e.to_string();
    // the bad value is the last `quoted` token before the list of accepted values
    let head = message.split("expected").next().unwrap_or_default();
    let Some(bad) = head.rsplit('`').nth(1) else {
        return e;
    };
    // Only for unknown values (serde's enums, `ColorRef`, `Aspect`): in other errors the
    // last quoted token may be the node id from the `metric node `lat`:` prefix.
    let before = head.rsplit_once(&format!("`{bad}`")).map_or("", |(b, _)| b);
    if !["variant ", "colour ", "aspect "]
        .iter()
        .any(|w| before.ends_with(w))
    {
        return e;
    }
    match find_string(&doc, bad, String::new()) {
        Some(path) => serde::de::Error::custom(format!("{path}: {message}")),
        None => e,
    }
}

/// Path of the first string value equal to `wanted`, skipping free-text keys.
fn find_string(v: &serde_json::Value, wanted: &str, path: String) -> Option<String> {
    use serde_json::Value;
    match v {
        Value::String(s) if s == wanted && !path.is_empty() => Some(path),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(i, item)| find_string(item, wanted, format!("{path}[{i}]"))),
        Value::Object(map) => map
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "id" | "name" | "text" | "type" | "$schema"))
            .find_map(|(k, item)| {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                find_string(item, wanted, p)
            }),
        _ => None,
    }
}

/// The bundled default layout.
pub fn default_layout() -> Layout {
    Layout::from_json(DEFAULT_LAYOUT_JSON)
        .expect("the bundled default layout is valid (tested)")
        .layout
}

/// The published JSON Schema of the layout format.
pub fn json_schema() -> serde_json::Value {
    schemars::schema_for!(Layout).to_value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use validate::Severity;

    #[test]
    fn layout_round_trips_unknown_keys_everywhere() {
        let input = json!({
            "$schema": "https://example.com/s.json", "version": 7, "future_top": [1, 2],
            "theme": {
                "font": "Roboto", "glow": 1,
                "palette": {"accent": "#ff0000", "tertiary": "#00ff00"},
                "outline": {"width": 2.0, "blur": 3},
                "shadow": {"offset": [1.0, 1.0], "spread": 5}
            },
            "nodes": [
                {"type": "frame", "size": [10.0, 10.0], "border": {"width": 1.0, "dash": [2, 2]}},
                {"type": "radar", "range": 50}
            ]
        });
        let layout: Layout = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(layout.version, 7);
        assert_eq!(layout.extra.len(), 1);
        let out = serde_json::to_value(&layout).unwrap();
        assert_eq!(out, input);
        let theme = layout.theme.unwrap();
        assert_eq!(theme.extra.get("glow"), Some(&json!(1)));
        assert_eq!(
            theme.palette.unwrap().extra.get("tertiary"),
            Some(&json!("#00ff00"))
        );
    }

    #[test]
    fn unknown_only_theme_groups_are_not_dropped() {
        let t: style::Theme = serde_json::from_value(json!({"outline": {"blur": 3}})).unwrap();
        assert_eq!(
            serde_json::to_value(&t).unwrap(),
            json!({"outline": {"blur": 3}})
        );
    }

    fn layout_with(nodes: serde_json::Value) -> String {
        json!({"version": 1, "nodes": nodes}).to_string()
    }

    fn errors(text: &str) -> Vec<String> {
        match Layout::from_json(text) {
            Err(LayoutError::Invalid(issues)) => {
                assert!(issues.iter().all(|i| i.severity == Severity::Error));
                issues.iter().map(ToString::to_string).collect()
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    fn json_error(text: &str) -> String {
        match Layout::from_json(text) {
            Err(LayoutError::Json(e)) => e.to_string(),
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn default_layout_loads_without_warnings() {
        let loaded = Layout::from_json(DEFAULT_LAYOUT_JSON).unwrap();
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.layout.nodes.len(), 4);
        assert_eq!(default_layout(), loaded.layout);
    }

    #[test]
    fn newer_version_loads_with_warning_and_keeps_unknown_parts() {
        let text = json!({
            "version": 2, "projection": "equirect",
            "nodes": [{"type": "moving_map", "id": "map", "zoom": 15},
                      {"type": "metric", "metric": "speed", "glow": 3}]
        })
        .to_string();
        let loaded = Layout::from_json(&text).unwrap();
        let w: Vec<String> = loaded.warnings.iter().map(ToString::to_string).collect();
        assert!(
            loaded
                .warnings
                .iter()
                .all(|i| i.severity == Severity::Warning)
        );
        assert!(w.iter().any(|m| m.contains("newer")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("projection")), "{w:?}");
        assert!(
            w.iter()
                .any(|m| m.contains("moving_map") && m.contains("map")),
            "{w:?}"
        );
        assert!(w.iter().any(|m| m.contains("glow")), "{w:?}");
        let saved = loaded.layout.to_json();
        for kept in ["moving_map", "projection", "glow", "\"zoom\": 15"] {
            assert!(saved.contains(kept), "{kept} lost on save:\n{saved}");
        }
    }

    #[test]
    fn unknown_keys_inside_style_groups_are_warnings() {
        let text = json!({
            "version": 1,
            "theme": {"palette": {"tertiary": "#00ff00"}, "outline": {"blur": 2},
                      "shadow": {"spread": 1}},
            "nodes": [
                {"type": "text", "id": "t", "text": "x", "outline": {"dash": 1}, "shadow": {"blur": 4}},
                {"type": "frame", "id": "f", "size": [10, 10], "border": {"style": "dotted"}}
            ]
        })
        .to_string();
        let loaded = Layout::from_json(&text).unwrap();
        let w: Vec<String> = loaded.warnings.iter().map(ToString::to_string).collect();
        assert_eq!(w.len(), 6, "{w:#?}");
        for (path, key) in [
            ("theme.palette", "tertiary"),
            ("theme.outline", "blur"),
            ("theme.shadow", "spread"),
            ("nodes[0] (t).outline", "dash"),
            ("nodes[0] (t).shadow", "blur"),
            ("nodes[1] (f).border", "style"),
        ] {
            assert!(
                w.iter()
                    .any(|m| m.starts_with(&format!("warning: {path}: unknown field `{key}`"))),
                "no warning for {path}.{key} in {w:#?}"
            );
        }
        // kept on save
        let saved = loaded.layout.to_json();
        for kept in ["tertiary", "spread", "dotted", "dash"] {
            assert!(saved.contains(kept), "{kept} lost on save:\n{saved}");
        }
    }

    #[test]
    fn errors_reject_layout_and_name_the_node() {
        let e = errors(&layout_with(json!([
            {"type": "text", "id": "a", "text": "x"},
            {"type": "text", "id": "a", "text": "y"}
        ])));
        assert!(
            e.iter()
                .any(|m| m.contains("duplicate id") && m.contains("nodes[1]")),
            "{e:?}"
        );

        let e = errors(&layout_with(json!([
            {"type": "metric", "id": "spd", "metric": "speed", "format": "{speed}"},
            {"type": "datetime", "format": "%Q"},
            {"type": "frame", "size": [0, 10], "opacity": 1.5},
            {"type": "metric", "metric": "", "stale_secs": -1}
        ])));
        let all = e.join("\n");
        for needle in [
            "nodes[0] (spd)",
            "format",
            "strftime",
            "size",
            "opacity",
            "metric",
            "stale_secs",
        ] {
            assert!(all.contains(needle), "missing `{needle}` in:\n{all}");
        }
    }

    #[test]
    fn errors_are_found_in_nested_children_and_theme() {
        let text = json!({
            "version": 1,
            "theme": {"dim_opacity": 2, "outline": {"width": -1}},
            "nodes": [{"type": "group", "id": "g", "size": [100, -5], "children": [
                {"type": "frame", "id": "f", "size": [10, 10], "radius": -2,
                 "border": {"width": -1}, "children": [
                    {"type": "icon", "id": "i", "icon": "", "size": 0},
                    {"type": "gps_lock_icon", "size": -3},
                    {"type": "text", "text": "x", "size": -1, "outline": {"width": -2}}
                ]}
            ]}]
        })
        .to_string();
        let e = errors(&text);
        let all = e.join("\n");
        for needle in [
            "theme: dim_opacity",
            "theme: outline width",
            "nodes[0] (g): size",
            "nodes[0] (g).children[0] (f): radius",
            "nodes[0] (g).children[0] (f): border width",
            "children[0] (i): icon must not be empty",
            "children[0] (i): size",
            "children[1]: size",
            "children[2]: size",
            "children[2]: outline width",
        ] {
            assert!(all.contains(needle), "missing `{needle}` in:\n{all}");
        }
        assert_eq!(e.len(), 10, "{all}");
    }

    #[test]
    fn non_finite_numbers_are_errors() {
        // JSON cannot spell NaN or infinity; layouts built in code (editor, importer) can.
        let mut layout = default_layout();
        let Node::Known(Widget::Frame(f)) = &mut layout.nodes[0] else {
            panic!("the default layout starts with a frame")
        };
        f.common.offset = Some([f32::NAN, 0.0]);
        f.common.opacity = Some(f32::NAN);
        f.size = [f32::INFINITY, 10.0];
        let Node::Known(Widget::Datetime(d)) = &mut f.children[0] else {
            panic!("the clock frame starts with the date")
        };
        d.style.shadow = Some(style::ShadowOpt {
            offset: Some([0.0, f32::NEG_INFINITY]),
            ..Default::default()
        });
        layout.theme = Some(style::Theme {
            shadow: Some(style::ShadowOpt {
                offset: Some([f32::NAN, 1.0]),
                ..Default::default()
            }),
            ..Default::default()
        });
        let issues = validate::validate(&layout);
        let all: Vec<String> = issues.iter().map(ToString::to_string).collect();
        let all = all.join("\n");
        for needle in [
            "error: nodes[0] (clock): offset must be finite",
            "error: nodes[0] (clock): opacity",
            "error: nodes[0] (clock): size must be a positive number (got inf)",
            "error: nodes[0] (clock).children[0] (date): shadow offset must be finite",
            "error: theme: shadow offset must be finite",
        ] {
            assert!(all.contains(needle), "missing `{needle}` in:\n{all}");
        }
        assert_eq!(issues.len(), 5, "{all}");
    }

    #[test]
    fn duplicate_ids_count_unknown_nodes_and_empty_ids_are_errors() {
        let e = errors(&layout_with(json!([
            {"type": "moving_map", "id": "m"},
            {"type": "group", "id": "", "children": [{"type": "text", "id": "m", "text": "x"}]}
        ])));
        let all = e.join("\n");
        assert!(all.contains("nodes[1]: id must not be empty"), "{all}");
        assert!(
            all.contains("nodes[1].children[0] (m): duplicate id `m`"),
            "{all}"
        );
    }

    #[test]
    fn unknown_enum_values_name_the_field_and_the_value() {
        let cases = [
            (
                layout_with(json!([{"type": "text", "id": "t", "text": "x", "anchor": "middle"}])),
                ["nodes[0].anchor", "middle"],
            ),
            (
                layout_with(json!([{"type": "text", "text": "x", "weight": "black"}])),
                ["nodes[0].weight", "black"],
            ),
            (
                layout_with(json!([{"type": "group", "children": [
                    {"type": "frame", "size": [1, 1], "border": {"color": "blue"}}]}])),
                ["nodes[0].children[0].border.color", "blue"],
            ),
            (
                json!({"version": 1, "units": "nautical", "nodes": []}).to_string(),
                ["units", "nautical"],
            ),
            (
                json!({"version": 1, "theme": {"outline": {"color": "pink"}}}).to_string(),
                ["theme.outline.color", "pink"],
            ),
        ];
        for (text, needles) in cases {
            let e = json_error(&text);
            for needle in needles {
                assert!(e.contains(needle), "missing `{needle}` in: {e}");
            }
        }
        // a wrong type is not an unknown value: no path is guessed, even when the node's
        // id (quoted in the message) equals another of its string fields
        let e = json_error(&layout_with(json!([
            {"type": "metric", "id": "lat", "metric": "lat", "size": "big"}
        ])));
        assert!(e.contains("size") || e.contains("f32"), "{e}");
        assert!(!e.contains(".metric:"), "wrong field named: {e}");
        // a syntax error is reported as is, with its position
        let e = json_error("{\"version\": 1,");
        assert!(e.contains("line 1"), "{e}");
    }

    #[test]
    fn version_is_required_and_positive() {
        assert!(matches!(
            Layout::from_json(r#"{"nodes": []}"#),
            Err(LayoutError::Json(_))
        ));
        let e = errors(r#"{"version": 0, "nodes": []}"#);
        assert!(e[0].contains("version"), "{e:?}");
    }

    #[test]
    fn invalid_error_lists_every_issue() {
        let err = Layout::from_json(&layout_with(json!([
            {"type": "text", "id": "a", "text": "x", "opacity": -1},
            {"type": "icon", "id": "b", "icon": ""}
        ])))
        .unwrap_err()
        .to_string();
        assert_eq!(
            err,
            "invalid layout:\n\
             error: nodes[0] (a): opacity must be between 0 and 1 (got -1)\n\
             error: nodes[1] (b): icon must not be empty"
        );
    }

    #[test]
    fn save_and_load_round_trip() {
        let path =
            std::env::temp_dir().join(format!("actionlay-{}-rt{FILE_SUFFIX}", std::process::id()));
        let layout = default_layout();
        layout.save(&path).unwrap();
        let loaded = Layout::load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(loaded.layout, layout);
        assert!(loaded.warnings.is_empty());
        assert!(matches!(Layout::load(&path), Err(LayoutError::Io(_))));
    }
}
