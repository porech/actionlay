//! Telemetry of action-camera videos: GoPro GPMF parsing, derived metrics,
//! and sampling at any file time. Pure Rust; packets come from the caller
//! (the media crate demuxes them), so this crate never links FFmpeg.
pub mod gpmf;
pub mod metric;
#[cfg(test)]
mod test_support;
pub mod units;
mod value;

pub use metric::Metric;
pub use value::{GpsLock, Value};
