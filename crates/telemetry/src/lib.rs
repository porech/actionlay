//! Telemetry of action-camera videos: GoPro GPMF parsing, derived metrics,
//! and sampling at any file time. Pure Rust; packets come from the caller
//! (the media crate demuxes them), so this crate never links FFmpeg.
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod extract;
pub mod gpmf;
#[allow(dead_code)] // used by telemetry.rs (Task 9)
mod lock;
pub mod metric;
#[allow(dead_code)] // used by derive.rs (Task 7)
mod smoothing;
#[cfg(test)]
mod test_support;
pub mod units;
mod value;

pub use extract::{Derived, GpsPoint};
pub use lock::LockOptions;
pub use metric::Metric;
pub use value::{GpsLock, Value};

/// One demuxed packet of the GoPro metadata stream, times in seconds of
/// file time. The media crate's `GpmfPacket` converts into this.
#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub pts: f64,
    pub duration: f64,
    pub data: Vec<u8>,
}
