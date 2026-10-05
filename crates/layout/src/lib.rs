//! ActionLay overlay layouts (`*.ovl.json`): model, defaults, validation, geometry.
//! Pure data: no dependency on other ActionLay crates (spec §2).
pub mod color;
pub mod format;
pub mod geom;
pub mod model;
pub mod style;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use model::{Extra, Node, Widget};

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
