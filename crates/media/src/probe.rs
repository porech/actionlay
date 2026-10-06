//! Static description of a media file.
use std::path::Path;

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;

use crate::{MediaError, color::ColorInfo, ffmpeg_info};

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub stream_index: usize,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub time_base: f64,
    pub color: ColorInfo,
    pub ten_bit: bool,
}

#[derive(Debug, Clone)]
pub struct AudioInfo {
    pub stream_index: usize,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub duration: f64,
    pub video: VideoInfo,
    pub audio: Option<AudioInfo>,
    pub telemetry: Option<TelemetryInfo>,
}

#[derive(Debug, Clone)]
pub struct TelemetryInfo {
    pub stream_index: usize,
    pub time_base: f64,
    pub packet_count: Option<usize>,
}

pub fn probe(path: &Path) -> Result<MediaInfo, MediaError> {
    ffmpeg_info::init();
    let _quiet = crate::gpmf::QuietLog::new();
    let input = ffmpeg::format::input(path)?;
    describe(&input)
}

pub(crate) fn describe(input: &ffmpeg::format::context::Input) -> Result<MediaInfo, MediaError> {
    let duration = input.duration().max(0) as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE);

    let vstream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or(MediaError::NoVideoStream)?;
    let vctx = ffmpeg::codec::Context::from_parameters(vstream.parameters())?;
    let vdec = vctx.decoder().video()?;
    let rate = vstream.avg_frame_rate();
    let pixel = vdec.format();
    let video = VideoInfo {
        stream_index: vstream.index(),
        codec: vdec
            .codec()
            .map(|c| c.name().to_string())
            .unwrap_or_default(),
        width: vdec.width(),
        height: vdec.height(),
        fps: if rate.denominator() == 0 {
            0.0
        } else {
            f64::from(rate)
        },
        time_base: f64::from(vstream.time_base()),
        color: ColorInfo::from_ffmpeg(vdec.color_space(), vdec.color_range(), pixel, vdec.height()),
        ten_bit: matches!(
            pixel,
            Pixel::YUV420P10LE | Pixel::YUV422P10LE | Pixel::P010LE
        ),
    };

    // No usable audio is not fatal: play the video without sound.
    let audio = input
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .and_then(|astream| {
            let opened = ffmpeg::codec::Context::from_parameters(astream.parameters())
                .and_then(|actx| actx.decoder().audio());
            match opened {
                Ok(adec) => Some(AudioInfo {
                    stream_index: astream.index(),
                    sample_rate: adec.rate(),
                    channels: adec.channels(),
                }),
                Err(e) => {
                    log::warn!(
                        "audio decoder unavailable for codec {:?}: {e}; playing without audio",
                        astream.parameters().id()
                    );
                    None
                }
            }
        });

    let telemetry = input
        .streams()
        .find(crate::gpmf::is_gpmd)
        .map(|s| TelemetryInfo {
            stream_index: s.index(),
            time_base: f64::from(s.time_base()),
            packet_count: usize::try_from(s.frames()).ok().filter(|n| *n > 0),
        });
    Ok(MediaInfo {
        duration,
        video,
        audio,
        telemetry,
    })
}
