//! Audio decoding (resampled to stereo f32) and output through cpal.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::{
    ChannelLayout, format::Sample, format::sample::Type as SampleType, frame, software::resampling,
};
use ringbuf::{HeapCons, HeapProd, HeapRb, traits::*};

use crate::{MediaError, clock::audio_clock_time};

pub struct AudioChunk {
    pub pts: f64,
    /// Interleaved stereo samples.
    pub samples: Vec<f32>,
}

pub struct AudioDecoder {
    decoder: ffmpeg::decoder::Audio,
    resampler: Option<resampling::Context>,
    time_base: f64,
    out_rate: u32,
}

impl AudioDecoder {
    pub fn open(
        params: ffmpeg::codec::Parameters,
        time_base: f64,
        out_rate: u32,
    ) -> Result<Self, MediaError> {
        let decoder = ffmpeg::codec::Context::from_parameters(params)?
            .decoder()
            .audio()?;
        Ok(Self {
            decoder,
            resampler: None,
            time_base,
            out_rate,
        })
    }

    pub fn send(&mut self, packet: &ffmpeg::Packet) -> Result<(), MediaError> {
        Ok(self.decoder.send_packet(packet)?)
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
        self.resampler = None;
    }

    /// Returns the next decoded chunk, or `None` when the decoder needs more input.
    pub fn receive(&mut self) -> Result<Option<AudioChunk>, MediaError> {
        loop {
            let mut decoded = frame::Audio::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {}
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::util::error::EAGAIN => {
                    return Ok(None);
                }
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(e) => return Err(e.into()),
            }
            let pts = decoded.timestamp().or(decoded.pts()).unwrap_or(0) as f64 * self.time_base;
            let resampler = match &mut self.resampler {
                Some(r) => r,
                slot => {
                    let layout = if decoded.channel_layout().is_empty() {
                        ChannelLayout::default(i32::from(decoded.channels()))
                    } else {
                        decoded.channel_layout()
                    };
                    slot.insert(resampling::Context::get(
                        decoded.format(),
                        layout,
                        decoded.rate(),
                        Sample::F32(SampleType::Packed),
                        ChannelLayout::STEREO,
                        self.out_rate,
                    )?)
                }
            };
            // ffmpeg-next sizes an empty output frame to the input sample count,
            // which truncates when upsampling (the excess piles up inside swr and
            // the audio drifts). Allocate the output with enough room instead.
            let delay = resampler.delay().map_or(0, |d| d.output.max(0)) as usize;
            let capacity = (decoded.samples() as u64 * u64::from(self.out_rate)
                / u64::from(decoded.rate().max(1))) as usize
                + delay
                + 64;
            let mut out = frame::Audio::new(
                Sample::F32(SampleType::Packed),
                capacity,
                ChannelLayout::STEREO,
            );
            resampler.run(&decoded, &mut out)?;
            let n = out.samples() * 2;
            if n == 0 {
                // The resampler is still buffering; try the next frame.
                continue;
            }
            let (words, _) = out.data(0)[..n * 4].as_chunks::<4>();
            let samples = words.iter().map(|b| f32::from_ne_bytes(*b)).collect();
            return Ok(Some(AudioChunk { pts, samples }));
        }
    }
}

struct Shared {
    frames_played: AtomicU64,
    latency_us: AtomicU64,
    muted: AtomicBool,
}

pub struct AudioOutput {
    _stream: cpal::Stream,
    producer: HeapProd<f32>,
    consumer_slot: Arc<Mutex<Option<HeapCons<f32>>>>,
    shared: Arc<Shared>,
    sample_rate: u32,
    base_pts: f64,
}

impl AudioOutput {
    pub const CAPACITY_SECONDS: f64 = 0.5;

    pub fn open() -> Result<Self, MediaError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| MediaError::Audio("no output device".into()))?;
        let supported = device
            .default_output_config()
            .map_err(|e| MediaError::Audio(e.to_string()))?;
        let sample_rate = supported.sample_rate();
        let mut config = supported.config();
        config.channels = 2;

        let (producer, consumer) =
            HeapRb::<f32>::new((f64::from(sample_rate) * 2.0 * Self::CAPACITY_SECONDS) as usize)
                .split();
        let consumer_slot = Arc::new(Mutex::new(Some(consumer)));
        let shared = Arc::new(Shared {
            frames_played: AtomicU64::new(0),
            latency_us: AtomicU64::new(0),
            muted: AtomicBool::new(false),
        });

        let slot = consumer_slot.clone();
        let sh = shared.clone();
        let stream = device
            .build_output_stream::<f32, _, _>(
                config,
                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    let ts = info.timestamp();
                    let lat = ts.playback.duration_since(ts.callback);
                    sh.latency_us
                        .store(lat.as_micros() as u64, Ordering::Relaxed);
                    // Muted: silence, buffer untouched, clock does not advance.
                    if sh.muted.load(Ordering::Relaxed) {
                        data.fill(0.0);
                        return;
                    }
                    let mut guard = slot.lock().unwrap();
                    let got = guard.as_mut().map(|c| c.pop_slice(data)).unwrap_or(0);
                    data[got..].fill(0.0);
                    sh.frames_played
                        .fetch_add((got / 2) as u64, Ordering::Relaxed);
                },
                |e| log::error!("audio stream error: {e}"),
                None,
            )
            .map_err(|e| MediaError::Audio(e.to_string()))?;
        stream
            .play()
            .map_err(|e| MediaError::Audio(e.to_string()))?;
        Ok(Self {
            _stream: stream,
            producer,
            consumer_slot,
            shared,
            sample_rate,
            base_pts: 0.0,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Queues interleaved stereo samples; returns how many were accepted.
    pub fn push(&mut self, samples: &[f32]) -> usize {
        self.producer.push_slice(samples)
    }

    pub fn queued_frames(&self) -> usize {
        self.producer.occupied_len() / 2
    }

    /// Drops queued audio and restarts the clock at `base_pts` (used on seek).
    pub fn reset(&mut self, base_pts: f64) {
        if let Some(c) = self.consumer_slot.lock().unwrap().as_mut() {
            c.clear();
        }
        self.shared.frames_played.store(0, Ordering::Relaxed);
        self.base_pts = base_pts;
    }

    /// Frames the output callback has consumed since the last reset. Unlike
    /// [`Self::clock`] it is not clamped by the output latency, so it shows
    /// whether the device is consuming at all.
    pub fn frames_played(&self) -> u64 {
        self.shared.frames_played.load(Ordering::Relaxed)
    }

    pub fn clock(&self) -> f64 {
        audio_clock_time(
            self.base_pts,
            self.shared.frames_played.load(Ordering::Relaxed),
            self.sample_rate,
            Duration::from_micros(self.shared.latency_us.load(Ordering::Relaxed)),
        )
    }

    /// Mutes or unmutes the output.
    ///
    /// Contract (the player relies on it): while muted the output callback emits
    /// silence, does NOT pop samples from the ring buffer and does NOT advance
    /// the played-frames counter, so the audio clock stands still and queued
    /// audio is preserved (pausing and resuming stays in sync). Only when
    /// unmuted does the callback pop samples, zero-fill any shortfall and count
    /// the popped frames.
    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn audio_output_is_send() {
        assert_send::<AudioOutput>();
    }
}
