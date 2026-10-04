//! Video decoding to NV12 frames, hardware first with software fallback.
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{format::Pixel, frame, software::scaling};

use crate::{
    MediaError,
    frame::{Nv12Frame, p010_to_nv12, pack_nv12, yuv420p_to_nv12, yuv420p10_to_nv12},
    hw::{self, HwKind},
};

pub struct VideoDecoder {
    decoder: ffmpeg::decoder::Video,
    time_base: f64,
    hw: Option<HwKind>,
    backend: &'static str,
    scaler: Option<((Pixel, u32, u32), scaling::Context)>,
    warned_sw_fallback: bool,
}

impl VideoDecoder {
    pub fn open(
        params: ffmpeg::codec::Parameters,
        time_base: f64,
        prefer_hw: bool,
    ) -> Result<Self, MediaError> {
        let want_hw = prefer_hw && std::env::var_os("ACTIONLAY_NO_HW").is_none();
        if let Some(kind) = HwKind::for_platform().filter(|_| want_hw) {
            match Self::open_hw(params.clone(), kind) {
                Ok(decoder) => {
                    return Ok(Self {
                        decoder,
                        time_base,
                        hw: Some(kind),
                        backend: "unknown",
                        scaler: None,
                        warned_sw_fallback: false,
                    });
                }
                Err(e) => log::warn!("hardware decoding unavailable, using software: {e}"),
            }
        }
        let mut ctx = ffmpeg::codec::Context::from_parameters(params)?;
        ctx.set_threading(ffmpeg::codec::threading::Config::kind(
            ffmpeg::codec::threading::Type::Frame,
        ));
        let decoder = ctx.decoder().video()?;
        Ok(Self {
            decoder,
            time_base,
            hw: None,
            backend: "software",
            scaler: None,
            warned_sw_fallback: false,
        })
    }

    fn open_hw(
        params: ffmpeg::codec::Parameters,
        kind: HwKind,
    ) -> Result<ffmpeg::decoder::Video, MediaError> {
        let mut ctx = ffmpeg::codec::Context::from_parameters(params)?;
        // SAFETY: the context is valid and not opened yet.
        unsafe { hw::attach(ctx.as_mut_ptr(), kind) }.map_err(MediaError::Hw)?;
        Ok(ctx.decoder().video()?)
    }

    pub fn active_backend(&self) -> &'static str {
        self.backend
    }

    pub fn send(&mut self, packet: &ffmpeg::Packet) -> Result<(), MediaError> {
        Ok(self.decoder.send_packet(packet)?)
    }

    pub fn send_eof(&mut self) -> Result<(), MediaError> {
        Ok(self.decoder.send_eof()?)
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
    }

    pub fn receive(&mut self) -> Result<Option<Nv12Frame>, MediaError> {
        let mut decoded = frame::Video::empty();
        match self.decoder.receive_frame(&mut decoded) {
            Ok(()) => {}
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                return Ok(None);
            }
            Err(ffmpeg::Error::Eof) => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        let ts = decoded.timestamp().or(decoded.pts()).unwrap_or(0);
        let pts = ts as f64 * self.time_base;
        let sw = if hw::is_hw_frame(&decoded) {
            self.backend = self.hw.map(HwKind::name).unwrap_or("unknown");
            hw::download(&decoded).map_err(MediaError::Hw)?
        } else {
            self.backend = "software";
            if let Some(kind) = self.hw
                && !self.warned_sw_fallback
            {
                self.warned_sw_fallback = true;
                log::warn!(
                    "{} hwaccel attached but the stream is being decoded in software (get_format fell back)",
                    kind.name()
                );
            }
            decoded
        };
        Ok(Some(self.convert_to_nv12(&sw, pts)?))
    }

    fn convert_to_nv12(&mut self, f: &frame::Video, pts: f64) -> Result<Nv12Frame, MediaError> {
        let (w, h) = (f.width(), f.height());
        let (y, uv) = match f.format() {
            Pixel::NV12 => pack_nv12(w, h, f.data(0), f.stride(0), f.data(1), f.stride(1)),
            Pixel::P010LE => p010_to_nv12(w, h, f.data(0), f.stride(0), f.data(1), f.stride(1)),
            Pixel::YUV420P | Pixel::YUVJ420P => yuv420p_to_nv12(
                w,
                h,
                f.data(0),
                f.stride(0),
                f.data(1),
                f.stride(1),
                f.data(2),
                f.stride(2),
            ),
            Pixel::YUV420P10LE => yuv420p10_to_nv12(
                w,
                h,
                f.data(0),
                f.stride(0),
                f.data(1),
                f.stride(1),
                f.data(2),
                f.stride(2),
            ),
            other => {
                let key = (other, w, h);
                let scaler = match &mut self.scaler {
                    Some((k, s)) if *k == key => s,
                    slot => {
                        let s = scaling::Context::get(
                            other,
                            w,
                            h,
                            Pixel::NV12,
                            w,
                            h,
                            scaling::Flags::BILINEAR,
                        )?;
                        &mut slot.insert((key, s)).1
                    }
                };
                let mut out = frame::Video::new(Pixel::NV12, w, h);
                scaler.run(f, &mut out)?;
                pack_nv12(w, h, out.data(0), out.stride(0), out.data(1), out.stride(1))
            }
        };
        Ok(Nv12Frame {
            width: w,
            height: h,
            y,
            uv,
            pts,
        })
    }
}
