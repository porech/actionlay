//! Embedded telemetry presets inspired by the upstream dashboards. Pixel resolution
//! variants share one responsive design. Map/chart variants will be added with M3.
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
    PRESETS.iter().find(|p| p.id == id)
}
