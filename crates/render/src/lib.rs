//! ActionLay overlay renderer: (layout, snapshot, size) → premultiplied RGBA (spec §4.5).
//! CPU only (tiny-skia), embedded fonts and icons: same output on every machine.
// Task 8 consumes everything; remove these allows then.
#[allow(dead_code)]
mod icons;
#[allow(dead_code)]
mod text;

pub use icons::IconId;
pub use text::FAMILY;
pub use tiny_skia;
