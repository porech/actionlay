//! Media layer of ActionLay: FFmpeg access, decoding, audio output, playback.
pub mod clock;
pub mod color;
mod error;
pub mod ffmpeg_info;
pub mod frame;
pub mod probe;

pub use error::MediaError;
