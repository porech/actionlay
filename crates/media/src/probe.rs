//! Static description of a media file.
use std::path::Path;

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;

use crate::{MediaError, color::ColorInfo, ffmpeg_info};

#[derive(Debug, Clone)]
pub struct VideoInfo {
    /// Display-matrix orientation, clockwise degrees (0, 90, 180, 270).
    pub rotation: u16,
    pub stream_index: usize,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub bit_rate: usize,
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
    pub creation_time: Option<String>,
    /// FFmpeg demuxer names and the ISO base-media major brand, when available.
    pub container: String,
    pub major_brand: Option<String>,
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
        rotation: stream_rotation(&vstream),
        stream_index: vstream.index(),
        codec: vdec
            .codec()
            .map(|c| c.name().to_string())
            .unwrap_or_default(),
        width: vdec.width(),
        height: vdec.height(),
        bit_rate: vdec.bit_rate(),
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
        creation_time: vstream
            .metadata()
            .get("creation_time")
            .map(str::to_owned)
            .or_else(|| input.metadata().get("creation_time").map(str::to_owned)),
        container: input.format().name().to_owned(),
        major_brand: input.metadata().get("major_brand").map(str::to_owned),
        duration,
        video,
        audio,
        telemetry,
    })
}

fn stream_rotation(stream: &ffmpeg::Stream<'_>) -> u16 {
    let Some(data) = stream
        .side_data()
        .find(|d| d.kind() == ffmpeg::codec::packet::side_data::Type::DisplayMatrix)
    else {
        return 0;
    };
    if data.data().len() < 36 {
        return 0;
    }
    // Copy into an aligned matrix; side-data byte pointers need not be aligned.
    let matrix: [i32; 9] = std::array::from_fn(|i| {
        i32::from_ne_bytes(data.data()[i * 4..i * 4 + 4].try_into().unwrap())
    });
    // SAFETY: matrix holds the nine native-endian elements required by FFmpeg.
    let angle = unsafe { ffmpeg::ffi::av_display_rotation_get(matrix.as_ptr()) };
    if !angle.is_finite() {
        return 0;
    }
    let clockwise = (-angle).rem_euclid(360.0);
    let quarter = (clockwise / 90.0).round();
    if (clockwise - quarter * 90.0).abs() > 1.0 {
        return 0;
    }
    ((quarter as u16) % 4) * 90
}
