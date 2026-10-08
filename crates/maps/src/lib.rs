//! Nonblocking raster tiles. Only requested visible tiles are fetched; no route prefetch.
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrivacyZone {
    pub lat: f64,
    pub lon: f64,
    pub radius_m: f64,
}

impl PrivacyZone {
    pub fn valid(&self) -> bool {
        self.lat.is_finite()
            && (-85.0..=85.0).contains(&self.lat)
            && self.lon.is_finite()
            && (-180.0..=180.0).contains(&self.lon)
            && self.radius_m.is_finite()
            && self.radius_m > 0.0
    }
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        self.crosses((lat, lon), (lat, lon))
    }
    pub fn crosses(&self, a: (f64, f64), b: (f64, f64)) -> bool {
        if !self.valid() {
            return false;
        }
        let project = |(lat, lon): (f64, f64)| {
            [
                ((lon - self.lon + 180.0).rem_euclid(360.0) - 180.0)
                    * 111_195.0
                    * self.lat.to_radians().cos(),
                (lat - self.lat) * 111_195.0,
            ]
        };
        let a = project(a);
        let b = project(b);
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = d[0] * d[0] + d[1] * d[1];
        let u = if len == 0.0 {
            0.0
        } else {
            (-(a[0] * d[0] + a[1] * d[1]) / len).clamp(0.0, 1.0)
        };
        (a[0] + u * d[0]).hypot(a[1] + u * d[1]) <= self.radius_m
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub online: bool,
    pub url: String,
    pub api_key: String,
    pub attribution: String,
    pub privacy: Vec<PrivacyZone>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            online: true,
            url: "https://tile.openstreetmap.org/{z}/{x}/{y}.png".into(),
            api_key: String::new(),
            attribution: "© OpenStreetMap contributors".into(),
            privacy: Vec::new(),
        }
    }
}
impl Settings {
    pub fn valid(&self) -> bool {
        reqwest::Url::parse(&self.url).is_ok_and(|u| matches!(u.scheme(), "https" | "http"))
            && ["{z}", "{x}", "{y}"].iter().all(|p| self.url.contains(p))
            && !self.attribution.trim().is_empty()
            && self.privacy.iter().all(PrivacyZone::valid)
    }
    fn provider(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.url.hash(&mut h);
        self.api_key.hash(&mut h);
        h.finish()
    }
    fn tile_url(&self, key: Tile) -> String {
        let api_key: String = self
            .api_key
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        self.url
            .replace("{z}", &key.z.to_string())
            .replace("{x}", &key.x.to_string())
            .replace("{y}", &key.y.to_string())
            .replace("{api_key}", &api_key)
    }
}

/// Normalised Web Mercator coordinates. X is periodic; Y is clamped at the poles.
pub fn project(lat: f64, lon: f64) -> [f64; 2] {
    let lat = lat.clamp(-85.05112878, 85.05112878).to_radians();
    [
        (lon + 180.0) / 360.0,
        (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0,
    ]
}
pub fn wrap_delta(delta: f64) -> f64 {
    (delta + 0.5).rem_euclid(1.0) - 0.5
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tile {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}
impl Tile {
    pub fn at(z: u8, x: i64, y: i64) -> Option<Self> {
        let n = 1_i64 << z.min(19);
        (y >= 0 && y < n).then_some(Self {
            z: z.min(19),
            x: x.rem_euclid(n) as u32,
            y: y as u32,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::TileStore;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::TileStore;
