//! Telemetry of action-camera videos: GoPro GPMF parsing, derived metrics,
//! and sampling at any file time. Pure Rust; packets come from the caller
//! (the media crate demuxes them), so this crate never links FFmpeg.
mod derive;
mod extract;
pub mod gpmf;
mod lock;
pub mod metric;
mod series;
mod smoothing;
mod telemetry;
#[cfg(test)]
mod test_support;
pub mod units;
mod value;
mod vehicle;

pub use extract::{Derived, GpsPoint};
pub use lock::LockOptions;
pub use metric::Metric;
pub use telemetry::{
    Availability, Snapshot, Telemetry, TelemetryError, TelemetryOptions, TrackPoint,
};
pub use value::{GpsLock, Value};

/// One demuxed packet of the GoPro metadata stream, times in seconds of
/// file time. The media crate's `GpmfPacket` converts into this.
#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub pts: f64,
    pub duration: f64,
    pub data: Vec<u8>,
}
