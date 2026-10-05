//! Colours as written in layouts: `#rrggbb` / `#rrggbbaa`, or a palette role.
use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Straight (not premultiplied) sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn parse_hex(s: &str) -> Option<Color> {
        let h = s.strip_prefix('#')?;
        if !(h.len() == 6 || h.len() == 8) || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Color {
            r: byte(0)?,
            g: byte(2)?,
            b: byte(4)?,
            a: if h.len() == 8 { byte(6)? } else { 255 },
        })
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Same colour with its alpha multiplied by `f` (clamped to 0..=1).
    pub fn with_alpha_mul(self, f: f32) -> Color {
        let a = (f32::from(self.a) * f.clamp(0.0, 1.0)).round() as u8;
        Color { a, ..self }
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse_hex(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid colour `{s}`: expected #rrggbb or #rrggbbaa"
            ))
        })
    }
}

impl JsonSchema for Color {
    fn schema_name() -> Cow<'static, str> {
        "Color".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$"
        })
    }
}

/// Palette entries of the layout theme (spec §4.3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Primary,
    Secondary,
    Accent,
    Panel,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Primary, Role::Secondary, Role::Accent, Role::Panel];

    pub fn id(self) -> &'static str {
        match self {
            Role::Primary => "primary",
            Role::Secondary => "secondary",
            Role::Accent => "accent",
            Role::Panel => "panel",
        }
    }

    pub fn from_id(id: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.id() == id)
    }
}

/// A colour in a widget: a palette role (restyled by the theme) or a fixed colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRef {
    Role(Role),
    Color(Color),
}

impl Serialize for ColorRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ColorRef::Role(r) => s.serialize_str(r.id()),
            ColorRef::Color(c) => c.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for ColorRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if let Some(r) = Role::from_id(&s) {
            return Ok(ColorRef::Role(r));
        }
        Color::parse_hex(&s).map(ColorRef::Color).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid colour `{s}`: expected a palette role (primary, secondary, accent, panel) or #rrggbb[aa]"
            ))
        })
    }
}

impl JsonSchema for ColorRef {
    fn schema_name() -> Cow<'static, str> {
        "ColorRef".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^(primary|secondary|accent|panel|#([0-9a-fA-F]{6}|[0-9a-fA-F]{8}))$"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints_hex() {
        assert_eq!(
            Color::parse_hex("#FF8000"),
            Some(Color::rgba(255, 128, 0, 255))
        );
        assert_eq!(
            Color::parse_hex("#ff800080"),
            Some(Color::rgba(255, 128, 0, 128))
        );
        assert_eq!(Color::rgba(255, 128, 0, 255).to_hex(), "#ff8000");
        assert_eq!(Color::rgba(255, 128, 0, 128).to_hex(), "#ff800080");
        for bad in ["ff8000", "#fff", "#gg8000", "red", "#ff80001", ""] {
            assert_eq!(Color::parse_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn alpha_multiplier_rounds_and_clamps() {
        let c = Color::rgba(10, 20, 30, 200);
        assert_eq!(c.with_alpha_mul(0.5).a, 100);
        assert_eq!(c.with_alpha_mul(2.0).a, 200);
        assert_eq!(c.with_alpha_mul(-1.0).a, 0);
    }

    #[test]
    fn color_ref_accepts_roles_and_hex() {
        let r: ColorRef = serde_json::from_str("\"accent\"").unwrap();
        assert_eq!(r, ColorRef::Role(Role::Accent));
        let c: ColorRef = serde_json::from_str("\"#00ff0080\"").unwrap();
        assert_eq!(c, ColorRef::Color(Color::rgba(0, 255, 0, 128)));
        assert_eq!(serde_json::to_string(&r).unwrap(), "\"accent\"");
        assert_eq!(serde_json::to_string(&c).unwrap(), "\"#00ff0080\"");
        let err = serde_json::from_str::<ColorRef>("\"blue\"")
            .unwrap_err()
            .to_string();
        assert!(err.contains("primary, secondary, accent, panel"), "{err}");
    }
}
