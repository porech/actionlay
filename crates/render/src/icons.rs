//! Embedded icons: Tabler Icons v3.48.0 (MIT, see assets/icons/LICENSE), rendered with
//! resvg into premultiplied pixmaps tinted with the widget colour, plus a dilated
//! "halo" silhouette used as an outline for legibility over bright video.
use std::collections::HashMap;

use actionlay_layout::color::Color;
use resvg::usvg;
use tiny_skia::{Pixmap, Transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconId {
    Speed,
    Altitude,
    Gradient,
    Distance,
    Temperature,
    Gps,
    GpsOff,
    Clock,
    Location,
    Heart,
    Power,
}

impl IconId {
    pub const ALL: [IconId; 11] = [
        IconId::Speed,
        IconId::Altitude,
        IconId::Gradient,
        IconId::Distance,
        IconId::Temperature,
        IconId::Gps,
        IconId::GpsOff,
        IconId::Clock,
        IconId::Location,
        IconId::Heart,
        IconId::Power,
    ];

    /// Name used in layouts (`"icon": "altitude"`).
    pub fn name(self) -> &'static str {
        match self {
            IconId::Speed => "speed",
            IconId::Altitude => "altitude",
            IconId::Gradient => "gradient",
            IconId::Distance => "distance",
            IconId::Temperature => "temperature",
            IconId::Gps => "gps",
            IconId::GpsOff => "gps-off",
            IconId::Clock => "clock",
            IconId::Location => "location",
            IconId::Heart => "heart",
            IconId::Power => "power",
        }
    }

    pub fn from_name(name: &str) -> Option<IconId> {
        IconId::ALL.into_iter().find(|i| i.name() == name)
    }

    fn svg(self) -> &'static str {
        match self {
            IconId::Speed => include_str!("../assets/icons/gauge.svg"),
            IconId::Altitude => include_str!("../assets/icons/mountain.svg"),
            IconId::Gradient => include_str!("../assets/icons/trending-up.svg"),
            IconId::Distance => include_str!("../assets/icons/route.svg"),
            IconId::Temperature => include_str!("../assets/icons/temperature.svg"),
            IconId::Gps => include_str!("../assets/icons/satellite.svg"),
            IconId::GpsOff => include_str!("../assets/icons/satellite-off.svg"),
            IconId::Clock => include_str!("../assets/icons/clock.svg"),
            IconId::Location => include_str!("../assets/icons/map-pin.svg"),
            IconId::Heart => include_str!("../assets/icons/heart.svg"),
            IconId::Power => include_str!("../assets/icons/power.svg"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    id: IconId,
    px: u32,
    rgba: [u8; 4],
    halo: u32,
}

/// Bound on cached pixmaps (sizes change while the window is resized).
const MAX_CACHED: usize = 256;

pub(crate) struct IconCache {
    trees: Vec<usvg::Tree>,
    cache: HashMap<Key, Pixmap>,
}

impl IconCache {
    pub fn new() -> Self {
        let options = usvg::Options::default();
        let trees = IconId::ALL
            .iter()
            .map(|id| {
                // Tabler strokes use currentColor; draw black and tint afterwards
                let svg = id.svg().replace("currentColor", "#000000");
                usvg::Tree::from_str(&svg, &options).expect("embedded icon parses")
            })
            .collect();
        Self {
            trees,
            cache: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// The icon `px` pixels wide tinted with `color`; with `halo > 0`, the silhouette
    /// dilated by `halo` pixels on every side (pixmap `px + 2·halo` wide).
    pub fn get(&mut self, id: IconId, px: u32, color: Color, halo: u32) -> Option<&Pixmap> {
        if px == 0 {
            return None;
        }
        let key = Key {
            id,
            px,
            rgba: [color.r, color.g, color.b, color.a],
            halo,
        };
        if !self.cache.contains_key(&key) {
            if self.cache.len() >= MAX_CACHED {
                self.cache.clear();
            }
            let pixmap = self.rasterize(id, px, color, halo)?;
            self.cache.insert(key, pixmap);
        }
        self.cache.get(&key)
    }

    fn rasterize(&self, id: IconId, px: u32, color: Color, halo: u32) -> Option<Pixmap> {
        let side = px + 2 * halo;
        let mut pixmap = Pixmap::new(side, side)?;
        let index = IconId::ALL.iter().position(|i| *i == id).expect("listed");
        let tree = &self.trees[index];
        let s = px as f32 / tree.size().width();
        let transform = Transform::from_row(s, 0.0, 0.0, s, halo as f32, halo as f32);
        resvg::render(tree, transform, &mut pixmap.as_mut());
        let mut alpha: Vec<u8> = pixmap
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[3])
            .collect();
        if halo > 0 {
            dilate(&mut alpha, side as usize, side as usize, halo as usize);
        }
        tint(&mut pixmap, &alpha, color);
        Some(pixmap)
    }
}

/// a × b / 255, rounded.
fn mul(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 127) / 255) as u8
}

/// Writes `color` × coverage as premultiplied RGBA.
fn tint(pixmap: &mut Pixmap, alpha: &[u8], c: Color) {
    let (r, g, b) = (mul(c.r, c.a), mul(c.g, c.a), mul(c.b, c.a));
    for (px, &a) in pixmap
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(alpha)
    {
        px[0] = mul(r, a);
        px[1] = mul(g, a);
        px[2] = mul(b, a);
        px[3] = mul(c.a, a);
    }
}

/// Square max filter of radius `r` (separable).
fn dilate(alpha: &mut [u8], w: usize, h: usize, r: usize) {
    let mut tmp = vec![0u8; w * h];
    for y in 0..h {
        let row = &alpha[y * w..(y + 1) * w];
        for x in 0..w {
            let (lo, hi) = (x.saturating_sub(r), (x + r).min(w - 1));
            tmp[y * w + x] = row[lo..=hi].iter().copied().max().unwrap_or(0);
        }
    }
    for x in 0..w {
        for y in 0..h {
            let (lo, hi) = (y.saturating_sub(r), (y + r).min(h - 1));
            alpha[y * w + x] = (lo..=hi).map(|yy| tmp[yy * w + x]).max().unwrap_or(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_a_name_and_parses() {
        let mut cache = IconCache::new();
        for id in IconId::ALL {
            assert_eq!(IconId::from_name(id.name()), Some(id));
            let pm = cache
                .get(id, 48, Color::rgba(255, 255, 255, 255), 0)
                .unwrap();
            assert!(
                pm.pixels().iter().any(|p| p.alpha() > 128),
                "{} is empty",
                id.name()
            );
        }
        assert_eq!(IconId::from_name("rocket"), None);
    }

    #[test]
    fn icons_are_tinted_and_premultiplied() {
        let mut cache = IconCache::new();
        let pm = cache
            .get(IconId::Speed, 48, Color::rgba(255, 0, 0, 255), 0)
            .unwrap();
        assert_eq!((pm.width(), pm.height()), (48, 48));
        for p in pm.pixels() {
            assert_eq!((p.red(), p.green(), p.blue()), (p.alpha(), 0, 0));
        }
        let half = cache
            .get(IconId::Speed, 48, Color::rgba(255, 0, 0, 128), 0)
            .unwrap();
        assert!(half.pixels().iter().all(|p| p.alpha() <= 128));
    }

    #[test]
    fn halo_is_a_larger_silhouette() {
        let mut cache = IconCache::new();
        let count = |pm: &Pixmap| pm.pixels().iter().filter(|p| p.alpha() > 0).count();
        let plain = count(
            cache
                .get(IconId::Altitude, 48, Color::rgba(0, 0, 0, 255), 0)
                .unwrap(),
        );
        let halo = cache
            .get(IconId::Altitude, 48, Color::rgba(0, 0, 0, 255), 3)
            .unwrap();
        assert_eq!((halo.width(), halo.height()), (54, 54));
        assert!(count(halo) > plain + 100);
    }

    #[test]
    fn zero_size_is_none() {
        assert!(
            IconCache::new()
                .get(IconId::Gps, 0, Color::rgba(0, 0, 0, 255), 0)
                .is_none()
        );
    }
}
