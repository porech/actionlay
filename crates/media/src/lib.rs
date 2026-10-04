//! Media layer of ActionLay: FFmpeg access, decoding, audio output, playback.
pub mod audio;
pub mod clock;
pub mod color;
mod error;
pub mod ffmpeg_info;
pub mod frame;
pub mod gpmf;
pub mod hw;
pub mod player;
pub mod present;
pub mod probe;
pub mod video;

pub use error::MediaError;
