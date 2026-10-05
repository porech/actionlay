//! Theme and style defaults (spec §4.3.2). Every optional field means "inherit":
//! widget-type default → layout theme → value set on the widget. Unset values are
//! not serialized, so "Reset to default" is removing the key.
//!
//! The model stores whatever is set, including a value equal to the inherited one.
//! The editor (milestone M4) is responsible for storing nothing when the user picks
//! a value equal to the inherited one.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::color::{Color, ColorRef, Role};
use crate::model::Extra;

// Groups with every field unset are not serialized either, so resetting the last
// field of a group leaves no empty object behind.
fn outline_unset(o: &Option<OutlineOpt>) -> bool {
    o.as_ref()
        .is_none_or(|o| o.color.is_none() && o.width.is_none() && o.extra.is_empty())
}

fn shadow_unset(o: &Option<ShadowOpt>) -> bool {
    o.as_ref()
        .is_none_or(|o| o.color.is_none() && o.offset.is_none() && o.extra.is_empty())
}

fn palette_unset(o: &Option<Palette>) -> bool {
    o.as_ref().is_none_or(|p| {
        p.primary.is_none()
            && p.secondary.is_none()
            && p.accent.is_none()
            && p.panel.is_none()
            && p.extra.is_empty()
    })
}

/// The look ActionLay ships with. Changing these restyles every layout that did not override them.
pub mod defaults {
    use crate::color::{Color, Role};

    pub const FONT: &str = "Roboto";
    pub const PRIMARY: Color = Color::rgba(0xff, 0xff, 0xff, 0xff);
    pub const SECONDARY: Color = Color::rgba(0xff, 0xff, 0xff, 0xcc);
    pub const ACCENT: Color = Color::rgba(0xff, 0xb3, 0x00, 0xff);
    pub const PANEL: Color = Color::rgba(0x0b, 0x0f, 0x14, 0x8c);
    pub const OUTLINE_COLOR: Color = Color::rgba(0x00, 0x00, 0x00, 0xd9);
    pub const OUTLINE_WIDTH: f32 = 3.0;
    pub const SHADOW_COLOR: Color = Color::rgba(0x00, 0x00, 0x00, 0x59);
    pub const SHADOW_OFFSET: [f32; 2] = [0.0, 2.0];
    pub const DIM_OPACITY: f32 = 0.45;
    pub const ICON_SIZE: f32 = 40.0;
    pub const ICON_ROLE: Role = Role::Accent;
    pub const FRAME_RADIUS: f32 = 14.0;
    pub const FRAME_FILL: Role = Role::Panel;
    pub const BORDER_ROLE: Role = Role::Accent;
    pub const BORDER_WIDTH: f32 = 0.0;
    pub const METRIC_FORMAT: &str = "{value:.0}";
    pub const DATETIME_FORMAT: &str = "%H:%M:%S";
    pub const STALE_SECS: f32 = 3.0;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FontWeight {
    Regular,
    Medium,
    Bold,
}

impl FontWeight {
    pub fn value(self) -> u16 {
        match self {
            FontWeight::Regular => 400,
            FontWeight::Medium => 500,
            FontWeight::Bold => 700,
        }
    }
}

/// Text outline (legibility over bright video). `width` in layout units; 0 disables it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OutlineOpt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    /// Keys this version does not know, preserved on save (spec §6.4).
    #[serde(flatten)]
    pub extra: Extra,
}

/// Hard drop shadow. `offset` in layout units.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShadowOpt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<[f32; 2]>,
    /// Keys this version does not know, preserved on save (spec §6.4).
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Palette {
    /// Main text colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<Color>,
    /// Labels and units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<Color>,
    /// Icons, highlights.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<Color>,
    /// Background of frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<Color>,
    /// Keys this version does not know, preserved on save (spec §6.4).
    #[serde(flatten)]
    pub extra: Extra,
}

/// Layout header theme (spec §4.1, §4.3.2).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Theme {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(default, skip_serializing_if = "palette_unset")]
    pub palette: Option<Palette>,
    #[serde(default, skip_serializing_if = "outline_unset")]
    pub outline: Option<OutlineOpt>,
    #[serde(default, skip_serializing_if = "shadow_unset")]
    pub shadow: Option<ShadowOpt>,
    /// Alpha multiplier of stale values and empty states (0..=1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dim_opacity: Option<f32>,
    /// Keys this version does not know, preserved on save (spec §6.4).
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlineStyle {
    pub color: Color,
    pub width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowStyle {
    pub color: Color,
    pub offset: [f32; 2],
}

/// Theme with every value filled in.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTheme {
    pub font: String,
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub panel: Color,
    pub outline: OutlineStyle,
    pub shadow: ShadowStyle,
    pub dim_opacity: f32,
}

impl Default for ResolvedTheme {
    fn default() -> Self {
        Theme::default().resolve()
    }
}

impl ResolvedTheme {
    pub fn color(&self, c: ColorRef) -> Color {
        match c {
            ColorRef::Role(Role::Primary) => self.primary,
            ColorRef::Role(Role::Secondary) => self.secondary,
            ColorRef::Role(Role::Accent) => self.accent,
            ColorRef::Role(Role::Panel) => self.panel,
            ColorRef::Color(c) => c,
        }
    }
}

impl Theme {
    pub fn resolve(&self) -> ResolvedTheme {
        let p = self.palette.clone().unwrap_or_default();
        let mut t = ResolvedTheme {
            font: self
                .font
                .clone()
                .unwrap_or_else(|| defaults::FONT.to_string()),
            primary: p.primary.unwrap_or(defaults::PRIMARY),
            secondary: p.secondary.unwrap_or(defaults::SECONDARY),
            accent: p.accent.unwrap_or(defaults::ACCENT),
            panel: p.panel.unwrap_or(defaults::PANEL),
            outline: OutlineStyle {
                color: defaults::OUTLINE_COLOR,
                width: defaults::OUTLINE_WIDTH,
            },
            shadow: ShadowStyle {
                color: defaults::SHADOW_COLOR,
                offset: defaults::SHADOW_OFFSET,
            },
            dim_opacity: self
                .dim_opacity
                .unwrap_or(defaults::DIM_OPACITY)
                .clamp(0.0, 1.0),
        };
        // the theme's own outline/shadow colours may refer to its palette
        t.outline = layer_outline(t.outline, self.outline.as_ref(), &t);
        t.shadow = layer_shadow(t.shadow, self.shadow.as_ref(), &t);
        t
    }
}

fn layer_outline(
    base: OutlineStyle,
    own: Option<&OutlineOpt>,
    theme: &ResolvedTheme,
) -> OutlineStyle {
    OutlineStyle {
        color: own
            .and_then(|o| o.color)
            .map_or(base.color, |c| theme.color(c)),
        width: own.and_then(|o| o.width).unwrap_or(base.width),
    }
}

fn layer_shadow(base: ShadowStyle, own: Option<&ShadowOpt>, theme: &ResolvedTheme) -> ShadowStyle {
    ShadowStyle {
        color: own
            .and_then(|s| s.color)
            .map_or(base.color, |c| theme.color(c)),
        offset: own.and_then(|s| s.offset).unwrap_or(base.offset),
    }
}

/// Text style group shared by text-like widgets; flattened into the widget's JSON object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextStyleOpt {
    /// Font size = height of the text box, in layout units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<FontWeight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(default, skip_serializing_if = "outline_unset")]
    pub outline: Option<OutlineOpt>,
    #[serde(default, skip_serializing_if = "shadow_unset")]
    pub shadow: Option<ShadowOpt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    Text,
    Metric,
    MetricUnit,
    Datetime,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeDefaults {
    pub size: f32,
    pub weight: FontWeight,
    pub color: Role,
}

impl TextKind {
    pub fn defaults(self) -> TypeDefaults {
        let (size, weight, color) = match self {
            TextKind::Text => (32.0, FontWeight::Regular, Role::Primary),
            TextKind::Metric => (64.0, FontWeight::Bold, Role::Primary),
            TextKind::MetricUnit => (28.0, FontWeight::Medium, Role::Secondary),
            TextKind::Datetime => (32.0, FontWeight::Medium, Role::Primary),
        };
        TypeDefaults {
            size,
            weight,
            color,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub font: String,
    pub size: f32,
    pub weight: FontWeight,
    pub color: Color,
    pub outline: OutlineStyle,
    pub shadow: ShadowStyle,
}

impl TextStyleOpt {
    pub fn resolve(&self, kind: TextKind, theme: &ResolvedTheme) -> TextStyle {
        let d = kind.defaults();
        TextStyle {
            font: self.font.clone().unwrap_or_else(|| theme.font.clone()),
            size: self.size.unwrap_or(d.size),
            weight: self.weight.unwrap_or(d.weight),
            color: theme.color(self.color.unwrap_or(ColorRef::Role(d.color))),
            outline: layer_outline(theme.outline, self.outline.as_ref(), theme),
            shadow: layer_shadow(theme.shadow, self.shadow.as_ref(), theme),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{Color, ColorRef, Role};

    const RED: Color = Color::rgba(255, 0, 0, 255);
    const GREEN: Color = Color::rgba(0, 255, 0, 255);

    fn red_theme() -> ResolvedTheme {
        Theme {
            palette: Some(Palette {
                primary: Some(RED),
                ..Default::default()
            }),
            ..Default::default()
        }
        .resolve()
    }

    #[test]
    fn type_defaults_apply_when_nothing_is_set() {
        let s = TextStyleOpt::default().resolve(TextKind::Metric, &ResolvedTheme::default());
        assert_eq!(s.size, 64.0);
        assert_eq!(s.weight, FontWeight::Bold);
        assert_eq!(s.color, defaults::PRIMARY);
        assert_eq!(s.font, "Roboto");
        assert_eq!(
            s.outline,
            OutlineStyle {
                color: defaults::OUTLINE_COLOR,
                width: defaults::OUTLINE_WIDTH
            }
        );
        let u = TextStyleOpt::default().resolve(TextKind::MetricUnit, &ResolvedTheme::default());
        assert_eq!(u.color, defaults::SECONDARY);
        assert_eq!(u.weight, FontWeight::Medium);
    }

    #[test]
    fn theme_restyles_widgets_that_did_not_override() {
        let s = TextStyleOpt::default().resolve(TextKind::Text, &red_theme());
        assert_eq!(s.color, RED);
        let accent = TextStyleOpt {
            color: Some(ColorRef::Role(Role::Accent)),
            ..Default::default()
        };
        let theme = Theme {
            palette: Some(Palette {
                accent: Some(GREEN),
                ..Default::default()
            }),
            ..Default::default()
        }
        .resolve();
        assert_eq!(accent.resolve(TextKind::Text, &theme).color, GREEN);
    }

    #[test]
    fn widget_values_win_over_theme_and_type_defaults() {
        let own = TextStyleOpt {
            size: Some(100.0),
            color: Some(ColorRef::Color(GREEN)),
            weight: Some(FontWeight::Regular),
            ..Default::default()
        };
        let s = own.resolve(TextKind::Metric, &red_theme());
        assert_eq!(
            (s.size, s.color, s.weight),
            (100.0, GREEN, FontWeight::Regular)
        );
    }

    #[test]
    fn outline_and_shadow_layer_per_field() {
        let theme = Theme {
            outline: Some(OutlineOpt {
                color: None,
                width: Some(4.0),
                ..Default::default()
            }),
            ..Default::default()
        }
        .resolve();
        // theme changes only the width: colour stays the shipped default
        assert_eq!(
            theme.outline,
            OutlineStyle {
                color: defaults::OUTLINE_COLOR,
                width: 4.0
            }
        );
        let own = TextStyleOpt {
            outline: Some(OutlineOpt {
                color: None,
                width: Some(0.0),
                ..Default::default()
            }),
            shadow: Some(ShadowOpt {
                color: Some(ColorRef::Role(Role::Accent)),
                offset: None,
                ..Default::default()
            }),
            ..Default::default()
        };
        let s = own.resolve(TextKind::Text, &theme);
        assert_eq!(s.outline.width, 0.0, "outline disabled on this widget only");
        assert_eq!(s.shadow.color, defaults::ACCENT);
        assert_eq!(s.shadow.offset, defaults::SHADOW_OFFSET);
    }

    #[test]
    fn only_set_values_are_serialized_so_reset_is_removing_the_key() {
        assert_eq!(
            serde_json::to_value(Theme::default()).unwrap(),
            serde_json::json!({})
        );
        assert_eq!(
            serde_json::to_value(TextStyleOpt::default()).unwrap(),
            serde_json::json!({})
        );
        let mut s = TextStyleOpt {
            color: Some(ColorRef::Role(Role::Accent)),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"color": "accent"})
        );
        s.color = None; // "Reset to default"
        assert_eq!(serde_json::to_value(&s).unwrap(), serde_json::json!({}));
    }

    #[test]
    fn theme_parses_from_json() {
        let t: Theme = serde_json::from_str(
            r##"{"font": "Roboto", "palette": {"accent": "#00c8ff"}, "dim_opacity": 0.3,
                 "outline": {"color": "#000000", "width": 3}}"##,
        )
        .unwrap();
        let r = t.resolve();
        assert_eq!(r.accent, Color::rgba(0, 200, 255, 255));
        assert_eq!(r.primary, defaults::PRIMARY);
        assert_eq!(r.dim_opacity, 0.3);
        assert_eq!(
            r.outline,
            OutlineStyle {
                color: Color::rgba(0, 0, 0, 255),
                width: 3.0
            }
        );
    }

    #[test]
    fn empty_groups_are_not_serialized() {
        let mut t = Theme {
            outline: Some(OutlineOpt {
                color: None,
                width: Some(4.0),
                ..Default::default()
            }),
            palette: Some(Palette {
                accent: Some(GREEN),
                ..Default::default()
            }),
            shadow: Some(ShadowOpt {
                color: None,
                offset: Some([1.0, 1.0]),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(serde_json::to_value(&t).unwrap().get("outline").is_some());
        t.outline.as_mut().unwrap().width = None;
        t.palette.as_mut().unwrap().accent = None;
        t.shadow.as_mut().unwrap().offset = None;
        assert_eq!(serde_json::to_value(&t).unwrap(), serde_json::json!({}));
        let own = TextStyleOpt {
            outline: Some(OutlineOpt::default()),
            ..Default::default()
        };
        assert_eq!(serde_json::to_value(&own).unwrap(), serde_json::json!({}));
    }

    #[test]
    fn font_layers_theme_then_widget() {
        let theme = Theme {
            font: Some("Inter".into()),
            ..Default::default()
        }
        .resolve();
        assert_eq!(
            TextStyleOpt::default().resolve(TextKind::Text, &theme).font,
            "Inter"
        );
        let own = TextStyleOpt {
            font: Some("Mono".into()),
            ..Default::default()
        };
        assert_eq!(own.resolve(TextKind::Text, &theme).font, "Mono");
    }

    #[test]
    fn shadow_colour_from_theme_keeps_type_default_offset() {
        let theme = Theme {
            shadow: Some(ShadowOpt {
                color: Some(ColorRef::Color(GREEN)),
                offset: None,
                ..Default::default()
            }),
            ..Default::default()
        }
        .resolve();
        let s = TextStyleOpt::default().resolve(TextKind::Metric, &theme);
        assert_eq!(
            s.shadow,
            ShadowStyle {
                color: GREEN,
                offset: defaults::SHADOW_OFFSET
            }
        );
    }

    #[test]
    fn theme_outline_role_follows_its_palette() {
        let theme = Theme {
            palette: Some(Palette {
                accent: Some(GREEN),
                ..Default::default()
            }),
            outline: Some(OutlineOpt {
                color: Some(ColorRef::Role(Role::Accent)),
                width: None,
                ..Default::default()
            }),
            ..Default::default()
        }
        .resolve();
        assert_eq!(theme.outline.color, GREEN);
    }

    #[test]
    fn text_and_datetime_type_defaults() {
        let t = TextStyleOpt::default().resolve(TextKind::Text, &ResolvedTheme::default());
        assert_eq!(
            (t.size, t.weight, t.color),
            (32.0, FontWeight::Regular, defaults::PRIMARY)
        );
        let d = TextStyleOpt::default().resolve(TextKind::Datetime, &ResolvedTheme::default());
        assert_eq!(
            (d.size, d.weight, d.color),
            (32.0, FontWeight::Medium, defaults::PRIMARY)
        );
    }

    #[test]
    fn populated_theme_and_style_round_trip() {
        let theme = Theme {
            font: Some("Inter".into()),
            palette: Some(Palette {
                primary: Some(RED),
                panel: Some(Color::rgba(1, 2, 3, 4)),
                ..Default::default()
            }),
            outline: Some(OutlineOpt {
                color: Some(ColorRef::Role(Role::Accent)),
                width: Some(1.5),
                ..Default::default()
            }),
            shadow: Some(ShadowOpt {
                color: Some(ColorRef::Color(GREEN)),
                offset: Some([1.0, 3.0]),
                ..Default::default()
            }),
            dim_opacity: Some(0.25),
            ..Default::default()
        };
        let json = serde_json::to_string(&theme).unwrap();
        assert!(json.contains("\"#ff0000\"") && json.contains("\"accent\""));
        assert_eq!(serde_json::from_str::<Theme>(&json).unwrap(), theme);

        let style = TextStyleOpt {
            size: Some(50.0),
            color: Some(ColorRef::Color(GREEN)),
            weight: Some(FontWeight::Medium),
            font: Some("Mono".into()),
            outline: Some(OutlineOpt {
                color: None,
                width: Some(0.0),
                ..Default::default()
            }),
            shadow: Some(ShadowOpt {
                color: Some(ColorRef::Role(Role::Panel)),
                offset: None,
                ..Default::default()
            }),
        };
        let json = serde_json::to_string(&style).unwrap();
        assert!(json.contains("\"weight\":\"medium\""));
        assert_eq!(serde_json::from_str::<TextStyleOpt>(&json).unwrap(), style);
    }
}
