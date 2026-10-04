//! FFmpeg initialisation and build metadata.
use std::{ffi::CStr, sync::Once};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::ffi;

#[derive(Debug, Clone)]
pub struct BuildInfo {
    pub version: String,
    pub configuration: String,
    pub license: String,
}

/// Initialises FFmpeg once per process and keeps its logging quiet.
pub fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        ffmpeg::init().expect("FFmpeg failed to initialise");
        ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Warning);
    });
}

pub fn build_info() -> BuildInfo {
    // SAFETY: these functions return pointers to static, NUL-terminated strings.
    unsafe {
        BuildInfo {
            version: CStr::from_ptr(ffi::av_version_info())
                .to_string_lossy()
                .into_owned(),
            configuration: CStr::from_ptr(ffi::avcodec_configuration())
                .to_string_lossy()
                .into_owned(),
            license: CStr::from_ptr(ffi::avcodec_license())
                .to_string_lossy()
                .into_owned(),
        }
    }
}
