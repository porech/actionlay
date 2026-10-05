//! Embedded telemetry presets inspired by the upstream dashboards. Pixel resolution
//! variants share one responsive design; the thirteen upstream XML conversions
//! are embedded alongside the native presets.
use crate::Layout;

pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub json: &'static str,
}

impl Preset {
    pub fn layout(&self) -> Layout {
        Layout::from_json(self.json)
            .expect("bundled presets are validated by tests")
            .layout
    }
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "default",
        name: "Default",
        description: "The original ActionLay dashboard",
        json: crate::DEFAULT_LAYOUT_JSON,
    },
    Preset {
        id: "moto",
        name: "Moto",
        description: "Speed, braking and acceleration, elevation and gradient",
        json: include_str!("../layouts/moto.ovl.json"),
    },
    Preset {
        id: "training",
        name: "Training",
        description: "Speed, heart-rate and power zones, cadence and elevation",
        json: include_str!("../layouts/training.ovl.json"),
    },
];

pub fn find(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().chain(UPSTREAM_PRESETS).find(|p| p.id == id)
}

/// XML conversions from the pinned GPL upstream revision; resolution variants are preserved.
pub const UPSTREAM_PRESETS: &[Preset] = &[
    Preset {
        id: "upstream-default-1920x1080",
        name: "default-1920x1080",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/default-1920x1080.ovl.json"),
    },
    Preset {
        id: "upstream-default-2688x1512",
        name: "default-2688x1512",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/default-2688x1512.ovl.json"),
    },
    Preset {
        id: "upstream-default-2704x1520",
        name: "default-2704x1520",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/default-2704x1520.ovl.json"),
    },
    Preset {
        id: "upstream-default-3840x2160",
        name: "default-3840x2160",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/default-3840x2160.ovl.json"),
    },
    Preset {
        id: "upstream-example-2",
        name: "example-2",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/example-2.ovl.json"),
    },
    Preset {
        id: "upstream-example",
        name: "example",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/example.ovl.json"),
    },
    Preset {
        id: "upstream-moto_1080",
        name: "moto_1080",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/moto_1080.ovl.json"),
    },
    Preset {
        id: "upstream-moto_1080_2bars",
        name: "moto_1080_2bars",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/moto_1080_2bars.ovl.json"),
    },
    Preset {
        id: "upstream-moto_1080_needle",
        name: "moto_1080_needle",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/moto_1080_needle.ovl.json"),
    },
    Preset {
        id: "upstream-moto_2160",
        name: "moto_2160",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/moto_2160.ovl.json"),
    },
    Preset {
        id: "upstream-moto_2160_2bars",
        name: "moto_2160_2bars",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/moto_2160_2bars.ovl.json"),
    },
    Preset {
        id: "upstream-moto_2160_needle",
        name: "moto_2160_needle",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/moto_2160_needle.ovl.json"),
    },
    Preset {
        id: "upstream-power-1920x1080",
        name: "power-1920x1080",
        description: "Upstream XML converted to native widgets; styling uses ActionLay equivalents",
        json: include_str!("../layouts/upstream/power-1920x1080.ovl.json"),
    },
];
