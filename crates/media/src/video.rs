//! Video decoding to NV12 frames, hardware first with software fallback.
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{format::Pixel, frame, software::scaling};
use std::time::Instant;

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
    timings_us: [u64; 3],
    late_frame: Option<(frame::Video, f64)>,
    decoded_frames: u64,
    skipped_frames: u64,
    output_seconds: f64,
    #[cfg(test)]
    pub(crate) output_delay_ms: u64,
}

pub(crate) enum PlaybackFrame {
    Frame(Nv12Frame),
    Skipped(f64),
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
                        timings_us: [0; 3],
                        late_frame: None,
                        decoded_frames: 0,
                        skipped_frames: 0,
                        output_seconds: 0.0,
                        #[cfg(test)]
                        output_delay_ms: 0,
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
            timings_us: [0; 3],
            late_frame: None,
            decoded_frames: 0,
            skipped_frames: 0,
            output_seconds: 0.0,
            #[cfg(test)]
            output_delay_ms: 0,
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
        let start = Instant::now();
        let result = self.decoder.send_packet(packet);
        self.timings_us[0] += start.elapsed().as_micros() as u64;
        Ok(result?)
    }

    pub fn send_eof(&mut self) -> Result<(), MediaError> {
        Ok(self.decoder.send_eof()?)
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
        self.late_frame = None;
    }

    pub fn receive(&mut self) -> Result<Option<Nv12Frame>, MediaError> {
        Ok(match self.receive_playback(f64::NEG_INFINITY)? {
            Some(PlaybackFrame::Frame(frame)) => Some(frame),
            _ => None,
        })
    }

    /// Decode references normally, but avoid downloading/converting output
    /// already behind the playback clock. Retain one raw frame for EOF, so a
    /// shorter video track still presents its final image.
    pub(crate) fn receive_playback(
        &mut self,
        earliest_pts: f64,
    ) -> Result<Option<PlaybackFrame>, MediaError> {
        let mut decoded = frame::Video::empty();
        let start = Instant::now();
        let result = self.decoder.receive_frame(&mut decoded);
        self.timings_us[0] += start.elapsed().as_micros() as u64;
        match result {
            Ok(()) => {}
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                return Ok(None);
            }
            Err(ffmpeg::Error::Eof) => {
                return self
                    .late_frame
                    .take()
                    .map(|(frame, pts)| self.materialize(frame, pts).map(PlaybackFrame::Frame))
                    .transpose();
            }
            Err(e) => return Err(e.into()),
        }
        let timestamp = decoded.timestamp().or(decoded.pts());
        let pts = timestamp.unwrap_or(0) as f64 * self.time_base;
        self.decoded_frames += 1;
        // Missing timestamps are not evidence that output is overdue.
        if timestamp.is_some() && pts.is_finite() && pts < earliest_pts {
            self.skipped_frames += 1;
            self.late_frame = Some((decoded, pts));
            return Ok(Some(PlaybackFrame::Skipped(pts)));
        }
        self.late_frame = None;
        Ok(Some(PlaybackFrame::Frame(self.materialize(decoded, pts)?)))
    }

    fn materialize(&mut self, decoded: frame::Video, pts: f64) -> Result<Nv12Frame, MediaError> {
        let output_started = Instant::now();
        #[cfg(test)]
        if self.output_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(self.output_delay_ms));
            self.timings_us[2] += output_started.elapsed().as_micros() as u64;
        }
        let sw = if hw::is_hw_frame(&decoded) {
            self.backend = self.hw.map(HwKind::name).unwrap_or("unknown");
            let start = Instant::now();
            let result = hw::download(&decoded).map_err(MediaError::Hw);
            self.timings_us[1] += start.elapsed().as_micros() as u64;
            result?
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
        let start = Instant::now();
        let result = self.convert_to_nv12(&sw, pts);
        self.timings_us[2] += start.elapsed().as_micros() as u64;
        // Only successful materialization predicts the next output cost.
        // Retain recent expensive copies, then decay as the backend recovers.
        if result.is_ok() {
            self.output_seconds = output_started
                .elapsed()
                .as_secs_f64()
                .max(self.output_seconds * 0.9);
        }
        result
    }

    /// The clock continues while output is downloaded and converted. Budget
    /// that cost before starting a copy rather than accepting a frame which
    /// will already be overdue when the copy completes.
    pub(crate) fn playback_cutoff(&self, clock: f64, allowed_lag: f64) -> f64 {
        clock + self.output_seconds - allowed_lag
    }

    pub(crate) fn frame_counts(&self) -> (u64, u64) {
        (self.decoded_frames, self.skipped_frames)
    }

    pub(crate) fn timings_us(&self) -> [u64; 3] {
        self.timings_us
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

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(cutoff: f64) -> (Vec<Nv12Frame>, (u64, u64)) {
        decode_with_timestamps(cutoff, true)
    }

    fn decode_with_timestamps(cutoff: f64, timestamps: bool) -> (Vec<Nv12Frame>, (u64, u64)) {
        ffmpeg::init().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../app/tests/fixtures/export-source.mp4");
        let mut input = ffmpeg::format::input(&path).unwrap();
        let stream = input.streams().best(ffmpeg::media::Type::Video).unwrap();
        let index = stream.index();
        let mut decoder =
            VideoDecoder::open(stream.parameters(), f64::from(stream.time_base()), false).unwrap();
        let mut frames = Vec::new();
        let drain = |decoder: &mut VideoDecoder, frames: &mut Vec<Nv12Frame>| {
            while let Some(output) = decoder.receive_playback(cutoff).unwrap() {
                if let PlaybackFrame::Frame(frame) = output {
                    frames.push(frame);
                }
            }
        };
        for (stream, mut packet) in input.packets() {
            if stream.index() == index {
                if !timestamps {
                    packet.set_pts(None);
                    packet.set_dts(None);
                }
                decoder.send(&packet).unwrap();
                drain(&mut decoder, &mut frames);
            }
        }
        decoder.send_eof().unwrap();
        drain(&mut decoder, &mut frames);
        (frames, decoder.frame_counts())
    }

    #[test]
    fn skipped_output_preserves_decoded_references() {
        let (all, baseline_counts) = decode(f64::NEG_INFINITY);
        assert!(all.len() > 10);
        let cutoff = all[all.len() / 2].pts;
        let (kept, counts) = decode(cutoff);
        let expected: Vec<_> = all.iter().filter(|f| f.pts >= cutoff).collect();
        assert_eq!(counts.0, baseline_counts.0);
        assert_eq!(counts.1 as usize, all.len() - expected.len());
        assert_eq!(kept.len(), expected.len());
        for (actual, expected) in kept.iter().zip(expected) {
            assert_eq!(actual.pts, expected.pts);
            assert_eq!(actual.y, expected.y);
            assert_eq!(actual.uv, expected.uv);
        }
    }

    #[test]
    fn skipped_output_still_presents_last_frame_at_eof() {
        let (all, _) = decode(f64::NEG_INFINITY);
        let (kept, counts) = decode(f64::INFINITY);
        assert_eq!(counts, (all.len() as u64, all.len() as u64));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].pts, all.last().unwrap().pts);
        assert_eq!(kept[0].y, all.last().unwrap().y);
        assert_eq!(kept[0].uv, all.last().unwrap().uv);
    }

    #[test]
    fn missing_timestamps_do_not_discard_output() {
        let (all, _) = decode(f64::NEG_INFINITY);
        let (kept, counts) = decode_with_timestamps(f64::INFINITY, false);
        assert_eq!(kept.len(), all.len());
        assert_eq!(counts.1, 0);
        for (actual, expected) in kept.iter().zip(all) {
            assert_eq!(actual.y, expected.y);
            assert_eq!(actual.uv, expected.uv);
        }
    }

    #[test]
    fn flushing_discards_deferred_frame_from_previous_seek() {
        ffmpeg::init().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../app/tests/fixtures/export-source.mp4");
        let mut input = ffmpeg::format::input(&path).unwrap();
        let stream = input.streams().best(ffmpeg::media::Type::Video).unwrap();
        let index = stream.index();
        let mut decoder =
            VideoDecoder::open(stream.parameters(), f64::from(stream.time_base()), false).unwrap();
        for (stream, packet) in input.packets() {
            if stream.index() == index {
                decoder.send(&packet).unwrap();
                if matches!(
                    decoder.receive_playback(f64::INFINITY).unwrap(),
                    Some(PlaybackFrame::Skipped(_))
                ) {
                    break;
                }
            }
        }
        assert!(decoder.late_frame.is_some());
        decoder.flush();
        decoder.send_eof().unwrap();
        assert!(decoder.receive().unwrap().is_none());
    }
}
