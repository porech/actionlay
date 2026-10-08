//! Offline decoding and encoding. The caller supplies straight RGBA frames.
use crate::{MediaError, frame::Nv12Frame, video::VideoDecoder};
use ffmpeg_next::{
    self as ffmpeg, Dictionary, Packet, Rational, codec, encoder,
    format::{self, Pixel},
    frame,
    software::scaling,
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn error(message: impl Into<String>) -> MediaError {
    MediaError::Io(message.into())
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RateControl {
    #[default]
    Input,
    Quality,
    Bitrate,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EncodingSpeed {
    Veryfast,
    #[default]
    Fast,
    Slow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EncodingSettings {
    pub rate_control: RateControl,
    pub quality: u8,
    pub bitrate_kbps: u32,
    pub max_bitrate_kbps: u32,
    pub buffer_kbits: u32,
    /// Zero leaves the encoder's GOP default intact.
    pub keyframe_frames: u32,
    pub speed: EncodingSpeed,
}

impl Default for EncodingSettings {
    fn default() -> Self {
        Self {
            rate_control: RateControl::Input,
            quality: 20,
            bitrate_kbps: 20_000,
            max_bitrate_kbps: 0,
            buffer_kbits: 0,
            keyframe_frames: 0,
            speed: EncodingSpeed::Fast,
        }
    }
}

impl EncodingSettings {
    pub fn valid(&self) -> bool {
        self.quality <= 51
            && (100..=500_000).contains(&self.bitrate_kbps)
            && self.max_bitrate_kbps <= 500_000
            && self.buffer_kbits <= 1_000_000
            && self.keyframe_frames <= 10_000
            && ((self.max_bitrate_kbps == 0 && self.buffer_kbits == 0)
                || (self.max_bitrate_kbps > 0 && self.buffer_kbits > 0))
            && (self.rate_control != RateControl::Bitrate
                || self.max_bitrate_kbps == 0
                || self.max_bitrate_kbps >= self.bitrate_kbps)
    }
}

#[derive(Debug, Clone)]
pub struct WriterOptions {
    pub hardware: bool,
    pub encoding: EncodingSettings,
    pub dimensions: Option<[u32; 2]>,
    /// RGBA canvas supplied to write(), after any video rotation.
    pub input_dimensions: Option<[u32; 2]>,
    pub container: &'static str,
}

fn keep_stream(input: &mut format::context::Input, index: usize) {
    for i in 0..input.nb_streams() as usize {
        if i != index {
            // SAFETY: i is within the live context's stream array.
            unsafe {
                (**(*input.as_mut_ptr()).streams.add(i)).discard =
                    ffmpeg::ffi::AVDiscard::AVDISCARD_ALL;
            }
        }
    }
}

/// Decodes every presentation frame, including delayed frames. No playback drops.
pub fn decode(
    path: &Path,
    start: f64,
    end: f64,
    cancel: Arc<AtomicBool>,
    mut emit: impl FnMut(Nv12Frame) -> Result<(), MediaError>,
) -> Result<(), MediaError> {
    let mut input = crate::input::open(path, cancel.clone())?;
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or(MediaError::NoVideoStream)?;
    let index = stream.index();
    let tb = f64::from(stream.time_base());
    let mut decoder = VideoDecoder::open(stream.parameters(), tb, true)?;
    let origin = if stream.start_time() == ffmpeg::ffi::AV_NOPTS_VALUE {
        0.0
    } else {
        stream.start_time() as f64 * tb
    };
    keep_stream(&mut input, index);
    if start > 0.0 {
        input.seek(
            ((start + origin) * 1_000_000.0) as i64,
            ..((start + origin) * 1_000_000.0) as i64,
        )?;
    }
    let mut done = false;
    loop {
        if cancel.load(Ordering::Relaxed) || done {
            break;
        }
        let mut packet = Packet::empty();
        match packet.read(&mut input) {
            Ok(()) if packet.stream() != index => continue,
            Ok(()) => decoder.send(&packet)?,
            Err(ffmpeg::Error::Eof) => {
                decoder.send_eof()?;
                done = true;
            }
            Err(e) if cancel.load(Ordering::Relaxed) => {
                log::debug!("export cancelled during read: {e}");
                break;
            }
            Err(e) => return Err(e.into()),
        }
        while let Some(mut frame) = decoder.receive()? {
            frame.pts -= origin;
            if frame.pts >= end {
                done = true;
                break;
            }
            if frame.pts + 1e-7 >= start {
                emit(frame)?;
            }
            if cancel.load(Ordering::Relaxed) {
                break;
            }
        }
    }
    Ok(())
}

pub struct Writer {
    output: format::context::Output,
    encoder: encoder::video::Encoder,
    scaler: scaling::Context,
    video_tb: Rational,
    encoder_tb: Rational,
    frame_duration: i64,
    audio: Option<AudioCopy>,
    start: f64,
    last_pts: Option<i64>,
    pub backend: String,
}
struct AudioCopy {
    input: format::context::Input,
    index: usize,
    tb: Rational,
    output_tb: Rational,
    origin: f64,
    next: Option<Packet>,
    ended: bool,
}

impl Writer {
    pub fn new(
        source: &Path,
        output: &Path,
        kind: &str,
        settings: &WriterOptions,
        start: f64,
        copy_audio: bool,
    ) -> Result<Self, MediaError> {
        crate::ffmpeg_info::init();
        if !settings.encoding.valid() {
            return Err(error("Invalid export encoding settings"));
        }
        let input = format::input(source)?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(MediaError::NoVideoStream)?;
        let decoder = codec::Context::from_parameters(stream.parameters())?
            .decoder()
            .video()?;
        let tb = stream.time_base();
        let fps = f64::from(stream.avg_frame_rate());
        if !fps.is_finite() || fps <= 0.0 {
            return Err(error("source has no valid frame rate"));
        }
        let frame_duration = ((1.0 / fps) / f64::from(tb)).round().max(1.0) as i64;
        let [width, height] = settings
            .dimensions
            .unwrap_or([decoder.width(), decoder.height()]);
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || width % 2 != 0
            || height % 2 != 0
        {
            return Err(error("Invalid export resolution"));
        }
        let mut output = format::output_as(output, settings.container)?;
        let global = output
            .format()
            .flags()
            .contains(format::Flags::GLOBAL_HEADER);
        let software = match kind {
            "h264" => "libx264",
            "h265" => "libx265",
            "prores" => "prores_ks",
            _ => return Err(error("unsupported export codec")),
        };
        let mut names = Vec::new();
        let encoding = settings.encoding;
        let bitrate = match encoding.rate_control {
            RateControl::Input => decoder.bit_rate(),
            RateControl::Bitrate => encoding.bitrate_kbps as usize * 1000,
            RateControl::Quality => 0,
        };
        if settings.hardware && kind != "prores" && bitrate > 0 {
            #[cfg(target_os = "macos")]
            names.push(if kind == "h264" {
                "h264_videotoolbox"
            } else {
                "hevc_videotoolbox"
            });
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            names.extend(if kind == "h264" {
                ["h264_nvenc", "h264_qsv", "h264_amf"]
            } else {
                ["hevc_nvenc", "hevc_qsv", "hevc_amf"]
            });
        }
        names.push(software);
        let mut opened = None;
        let pixel = if kind == "prores" {
            Pixel::YUVA444P10LE
        } else {
            Pixel::YUV420P
        };
        for name in names {
            let Some(codec) = encoder::find_by_name(name) else {
                continue;
            };
            let mut enc = codec::Context::new_with_codec(codec).encoder().video()?;
            enc.set_width(width);
            enc.set_height(height);
            enc.set_format(pixel);
            enc.set_time_base(tb);
            enc.set_frame_rate(Some(stream.avg_frame_rate()));
            enc.set_aspect_ratio(decoder.aspect_ratio());
            enc.set_colorspace(ffmpeg::color::Space::BT709);
            enc.set_color_range(ffmpeg::color::Range::MPEG);
            enc.set_max_b_frames(0);
            enc.set_bit_rate(bitrate);
            if kind != "prores" {
                if encoding.keyframe_frames > 0 {
                    enc.set_gop(encoding.keyframe_frames);
                }
                enc.set_max_bit_rate(encoding.max_bitrate_kbps as usize * 1000);
                // SAFETY: live encoder context, bounded to fit the i32 FFmpeg field.
                unsafe {
                    (*enc.as_mut_ptr()).rc_buffer_size = (encoding.buffer_kbits * 1000) as i32;
                }
            }
            enc.set_threading(codec::threading::Config::kind(
                codec::threading::Type::Frame,
            ));
            if global {
                enc.set_flags(codec::Flags::GLOBAL_HEADER);
            }
            let mut options = Dictionary::new();
            if kind == "prores" {
                options.set("profile", "4");
                options.set("alpha_bits", "16");
            } else if name == software {
                options.set(
                    "preset",
                    match encoding.speed {
                        EncodingSpeed::Veryfast => "veryfast",
                        EncodingSpeed::Fast => "fast",
                        EncodingSpeed::Slow => "slow",
                    },
                );
                if bitrate == 0 {
                    options.set("crf", &encoding.quality.to_string());
                }
            } else if name.ends_with("_nvenc") {
                options.set(
                    "preset",
                    match encoding.speed {
                        EncodingSpeed::Veryfast => "p2",
                        EncodingSpeed::Fast => "p4",
                        EncodingSpeed::Slow => "p7",
                    },
                );
                options.set("rc", "vbr");
            } else if name.ends_with("_videotoolbox") {
                options.set(
                    "realtime",
                    if encoding.speed == EncodingSpeed::Veryfast {
                        "1"
                    } else {
                        "0"
                    },
                );
            }
            match enc.open_with(options) {
                Ok(enc) => {
                    opened = Some((enc, name.to_string()));
                    break;
                }
                Err(e) => log::warn!("export encoder {name} unavailable: {e}"),
            }
        }
        let (encoder, backend) =
            opened.ok_or_else(|| error(format!("no usable {kind} encoder in this build")))?;
        let mut video = output.add_stream(encoder::find_by_name(&backend))?;
        video.set_time_base(tb);
        video.set_parameters(&encoder);
        let audio = if copy_audio {
            if let Some(astream) = input.streams().best(ffmpeg::media::Type::Audio) {
                let index = astream.index();
                let atb = astream.time_base();
                let origin = if stream.start_time() == ffmpeg::ffi::AV_NOPTS_VALUE {
                    0.0
                } else {
                    stream.start_time() as f64 * f64::from(tb)
                };
                let mut audio_stream = output.add_stream(encoder::find(codec::Id::None))?;
                audio_stream.set_parameters(astream.parameters());
                audio_stream.set_time_base(atb);
                // SAFETY: live stream parameters, clear the source container's tag.
                unsafe {
                    (*audio_stream.parameters().as_mut_ptr()).codec_tag = 0;
                }
                let mut audio_input = format::input(source)?;
                keep_stream(&mut audio_input, index);
                if start > 0.0 {
                    audio_input.seek(
                        ((start + origin) * 1_000_000.0) as i64,
                        ..((start + origin) * 1_000_000.0) as i64,
                    )?;
                }
                Some(AudioCopy {
                    input: audio_input,
                    index,
                    tb: atb,
                    output_tb: atb,
                    origin,
                    next: None,
                    ended: false,
                })
            } else {
                None
            }
        } else {
            None
        };
        output.write_header()?;
        let video_tb = output
            .stream(0)
            .ok_or_else(|| error("output video stream missing"))?
            .time_base();
        let mut audio = audio;
        if let Some(a) = &mut audio {
            a.output_tb = output
                .stream(1)
                .ok_or_else(|| error("output audio stream missing"))?
                .time_base();
        }
        let mut scaler = scaling::Context::get(
            Pixel::RGBA,
            settings
                .input_dimensions
                .unwrap_or([decoder.width(), decoder.height()])[0],
            settings
                .input_dimensions
                .unwrap_or([decoder.width(), decoder.height()])[1],
            pixel,
            width,
            height,
            scaling::Flags::BILINEAR,
        )?;
        // SAFETY: live swscale context and static coefficient table; RGBA is
        // full range, the encoded YUV is limited-range BT.709.
        unsafe {
            let table = ffmpeg::ffi::sws_getCoefficients(1);
            if ffmpeg::ffi::sws_setColorspaceDetails(
                scaler.as_mut_ptr(),
                table,
                1,
                table,
                0,
                0,
                1 << 16,
                1 << 16,
            ) < 0
            {
                return Err(error("cannot configure export colours"));
            }
        }
        Ok(Self {
            output,
            encoder,
            scaler,
            video_tb,
            encoder_tb: tb,
            frame_duration,
            audio,
            start,
            last_pts: None,
            backend,
        })
    }

    pub fn write(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        t: f64,
    ) -> Result<(), MediaError> {
        if rgba.len() != width as usize * height as usize * 4 {
            return Err(error("invalid RGBA frame size"));
        }
        let pts = ((t - self.start) / f64::from(self.encoder_tb)).round() as i64;
        if self.last_pts.is_some_and(|last| pts <= last) {
            return Err(error("non-increasing video timestamps"));
        }
        let mut rgb = frame::Video::new(Pixel::RGBA, width, height);
        let stride = rgb.stride(0);
        for (row, pixels) in rgba.chunks_exact(width as usize * 4).enumerate() {
            rgb.data_mut(0)[row * stride..row * stride + pixels.len()].copy_from_slice(pixels);
        }
        let mut converted = frame::Video::empty();
        self.scaler.run(&rgb, &mut converted)?;
        converted.set_pts(Some(pts));
        self.encoder.send_frame(&converted)?;
        self.drain()?;
        self.copy_audio(t)?;
        self.last_pts = Some(pts);
        Ok(())
    }
    fn drain(&mut self) -> Result<(), MediaError> {
        loop {
            let mut packet = Packet::empty();
            match self.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    packet.set_stream(0);
                    if packet.duration() == 0 {
                        packet.set_duration(self.frame_duration);
                    }
                    packet.rescale_ts(self.encoder_tb, self.video_tb);
                    packet.write_interleaved(&mut self.output)?;
                }
                Err(ffmpeg::Error::Eof) => break,
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    fn copy_audio(&mut self, until: f64) -> Result<(), MediaError> {
        let Some(a) = &mut self.audio else {
            return Ok(());
        };
        loop {
            let packet = if let Some(packet) = a.next.take() {
                packet
            } else {
                if a.ended {
                    break;
                }
                let mut packet = Packet::empty();
                match packet.read(&mut a.input) {
                    Ok(()) => {}
                    Err(ffmpeg::Error::Eof) => {
                        a.ended = true;
                        break;
                    }
                    Err(e) => return Err(e.into()),
                }
                if packet.stream() != a.index {
                    continue;
                }
                packet
            };
            let t = packet
                .pts()
                .or(packet.dts())
                .ok_or_else(|| error("audio packet has no timestamp"))? as f64
                * f64::from(a.tb)
                - a.origin;
            if t < self.start {
                continue;
            }
            if t >= until {
                a.next = Some(packet);
                break;
            }
            let mut packet = packet;
            let shift = ((self.start + a.origin) / f64::from(a.tb)).round() as i64;
            packet.set_pts(packet.pts().map(|p| p - shift));
            packet.set_dts(packet.dts().map(|p| p - shift));
            packet.set_stream(1);
            packet.set_position(-1);
            packet.rescale_ts(a.tb, a.output_tb);
            packet.write_interleaved(&mut self.output)?;
        }
        Ok(())
    }
    pub fn finish(mut self, until: f64) -> Result<(), MediaError> {
        self.encoder.send_eof()?;
        self.drain()?;
        self.copy_audio(until)?;
        self.output.write_trailer()?;
        Ok(())
    }
}

/// Matches the preview shader's NV12 colour conversion before alpha compositing.
pub fn rgba(frame: &Nv12Frame, color: crate::color::ColorInfo) -> Vec<u8> {
    let m = crate::color::yuv_to_rgb(color);
    let w = frame.width as usize;
    let mut rgba = vec![255; w * frame.height as usize * 4];
    for (i, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let uv = (i / w / 2) * w + (i % w / 2) * 2;
        let y = frame.y[i] as f32 / 255.0;
        let u = frame.uv[uv] as f32 / 255.0;
        let v = frame.uv[uv + 1] as f32 / 255.0;
        for c in 0..3 {
            pixel[c] = ((m[c][0] * y + m[c][1] * u + m[c][2] * v + m[c][3]).clamp(0.0, 1.0) * 255.0)
                .round() as u8;
        }
    }
    rgba
}
