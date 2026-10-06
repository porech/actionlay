//! ActionLay overlay layouts (`*.ovl.json`): model, defaults, validation, geometry.
//! Pure data: no dependency on other ActionLay crates (spec §2).
pub mod catalog;
pub mod color;
pub mod format;
pub mod geom;
pub mod import;
pub mod model;
pub mod package;
pub mod scale;
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
    /// Assets scoped to this document, never installed or serialized as bytes.
    #[serde(skip)]
    #[schemars(skip)]
    pub loaded_assets: std::collections::BTreeMap<String, std::sync::Arc<Vec<u8>>>,
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Format version, 1 or higher.
    #[schemars(range(min = 1))]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Default 16:9; used by the `fit` scale mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_aspect: Option<geom::Aspect>,
    /// Measurement units
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
        let (errors, warnings) = split(validate::validate(&layout));
        if errors.is_empty() {
            Ok(Loaded { layout, warnings })
        } else {
            Err(LayoutError::Invalid(errors))
        }
    }

    pub fn load(path: &Path) -> Result<Loaded, LayoutError> {
        if package::is_package(path) {
            return package::load(path);
        }
        let mut loaded = Self::from_json(&std::fs::read_to_string(path)?)?;
        package::load_loose_assets(&mut loaded.layout, path.parent().unwrap_or(Path::new(".")))?;
        Ok(loaded)
    }

    /// The layout as pretty JSON. A layout with errors is refused, so that nothing is
    /// written that cannot be loaded back.
    pub fn to_json(&self) -> Result<String, LayoutError> {
        let (errors, _) = split(validate::validate(self));
        if !errors.is_empty() {
            return Err(LayoutError::Invalid(errors));
        }
        Ok(serde_json::to_string_pretty(self).expect("a layout always serializes"))
    }

    /// Writes the layout; refuses a layout with errors (see [`Layout::to_json`]).
    pub fn save(&self, path: &Path) -> Result<(), LayoutError> {
        if package::is_package(path) {
            return package::save(self, path);
        }
        if !self.loaded_assets.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Save asset-bearing layouts as .actionlay-layout",
            )
            .into());
        }
        std::fs::write(path, self.to_json()? + "\n")?;
        Ok(())
    }
}

/// (errors, warnings)
fn split(issues: Vec<Issue>) -> (Vec<Issue>, Vec<Issue>) {
    issues
        .into_iter()
        .partition(|i| i.severity == Severity::Error)
}

/// serde names a bad value (`unknown variant `middle``, `invalid colour `blue``) but,
/// through the flattened structs of the model, not the key that holds it. Put the path
/// of that key in front of the message when it can be told for sure:
/// `nodes[0].anchor: text node `t`: unknown variant `middle`, expected one of …`.
/// Errors in nodes are located by the failing node itself ([`model::node_error`]);
/// errors in the header by the only header key holding the bad value.
fn name_the_field(text: &str, e: serde_json::Error) -> serde_json::Error {
    if !e.is_data() {
        return e;
    }
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return e;
    };
    let position = format!(" at line {} column {}", e.line(), e.column());
    if let Some(nodes) = doc.get("nodes").and_then(|n| n.as_array()) {
        for (i, node) in nodes.iter().enumerate() {
            if let Some((path, message)) = model::node_error(node) {
                let path = if path.is_empty() {
                    format!("nodes[{i}]")
                } else {
                    format!("nodes[{i}].{path}")
                };
                return serde::de::Error::custom(model::at_path(&path, &message) + &position);
            }
        }
    }
    let message = e.to_string();
    match model::bad_value(&message).and_then(|bad| model::unique_key(&doc, bad, &["nodes"])) {
        Some(path) => serde::de::Error::custom(model::at_path(&path, &message)),
        None => e,
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
    let mut schema = schemars::schema_for!(Layout).to_value();
    schema.sort_all_objects();
    schema
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
        let saved = loaded.layout.to_json().unwrap();
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
        let saved = loaded.layout.to_json().unwrap();
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
        let text = |nodes: serde_json::Value| layout_with(nodes);
        let cases = [
            (
                text(json!([{"type": "text", "id": "t", "text": "x", "anchor": "middle"}])),
                "nodes[0].anchor: text node `t`: unknown variant `middle`, expected one of",
            ),
            (
                text(json!([{"type": "text", "text": "x", "weight": "black"}])),
                "nodes[0].weight: text node: unknown variant `black`",
            ),
            // nested child: the path goes through the parents
            (
                text(json!([{"type": "group", "id": "g", "children": [
                    {"type": "text", "text": "ok", "anchor": "top"},
                    {"type": "frame", "id": "f", "size": [1, 1], "children": [
                        {"type": "frame", "size": [1, 1], "border": {"color": "blue"}}]}]}])),
                "nodes[0].children[1].children[0].border.color: frame node: invalid colour `blue`",
            ),
            (
                json!({"version": 1, "units": "nautical", "nodes": []}).to_string(),
                "units: unknown variant `nautical`",
            ),
            (
                json!({"version": 1, "design_aspect": "4x3"}).to_string(),
                "design_aspect: invalid aspect `4x3`",
            ),
            (
                json!({"version": 1, "theme": {"outline": {"color": "pink"}}}).to_string(),
                "theme.outline.color: invalid colour `pink`",
            ),
            // the same string in a valid key of another node is not searched
            (
                text(json!([
                    {"type": "text", "text": "a", "color": "secondary"},
                    {"type": "text", "id": "b", "text": "b", "weight": "secondary"}
                ])),
                "nodes[1].weight: text node `b`: unknown variant `secondary`",
            ),
            // two keys of the failing node hold the bad value: no field is guessed
            (
                text(json!([{"type": "text", "text": "a", "anchor": "top", "color": "top"}])),
                "nodes[0]: text node: invalid colour `top`",
            ),
            // unknown nodes are kept verbatim and never searched
            (
                text(json!([
                    {"type": "moving_map", "tint": "blue"},
                    {"type": "text", "text": "a", "color": "blue"}
                ])),
                "nodes[1].color: text node: invalid colour `blue`",
            ),
            // a wrong type is not an unknown value: no field is guessed, even when the
            // node id (quoted in the message) equals another of its string fields
            (
                text(json!([{"type": "metric", "id": "lat", "metric": "lat", "size": "big"}])),
                "nodes[0]: metric node `lat`: invalid type: string \"big\", expected f32",
            ),
        ];
        for (text, expected) in cases {
            let e = json_error(&text);
            assert!(e.starts_with(expected), "expected `{expected}…`, got: {e}");
        }
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

    #[test]
    fn layouts_with_errors_are_not_written() {
        let path = std::env::temp_dir().join(format!(
            "actionlay-{}-invalid{FILE_SUFFIX}",
            std::process::id()
        ));
        let mut layout = default_layout();
        layout.version = 0;
        let Node::Known(Widget::Frame(f)) = &mut layout.nodes[1] else {
            panic!("the second default node is a frame")
        };
        f.common.opacity = Some(2.0);
        let Err(LayoutError::Invalid(issues)) = layout.to_json() else {
            panic!("to_json accepted an invalid layout")
        };
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(matches!(layout.save(&path), Err(LayoutError::Invalid(_))));
        assert!(!path.exists(), "an invalid layout was written");
        // warnings do not block saving
        let mut layout = default_layout();
        layout.extra.insert("future".into(), json!(1));
        assert!(layout.to_json().unwrap().contains("\"future\": 1"));
    }
}
