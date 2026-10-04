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
}

pub fn probe(path: &Path) -> Result<MediaInfo, MediaError> {
    ffmpeg_info::init();
    let input = ffmpeg::format::input(path)?;
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

    let audio = match input.streams().best(ffmpeg::media::Type::Audio) {
        Some(astream) => {
            let actx = ffmpeg::codec::Context::from_parameters(astream.parameters())?;
            let adec = actx.decoder().audio()?;
            Some(AudioInfo {
                stream_index: astream.index(),
                sample_rate: adec.rate(),
                channels: adec.channels(),
            })
        }
        None => None,
    };

    Ok(MediaInfo {
        duration,
        video,
        audio,
    })
}
