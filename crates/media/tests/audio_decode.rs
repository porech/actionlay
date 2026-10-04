mod common;
use actionlay_media::{audio::AudioDecoder, probe::probe};
use ffmpeg_next as ffmpeg;

fn decode_all(path: &std::path::Path, out_rate: u32) -> Vec<actionlay_media::audio::AudioChunk> {
    let info = probe(path).unwrap();
    let a = info.audio.unwrap();
    let mut input = ffmpeg::format::input(path).unwrap();
    let stream = input.stream(a.stream_index).unwrap();
    let mut dec =
        AudioDecoder::open(stream.parameters(), f64::from(stream.time_base()), out_rate).unwrap();
    let mut chunks = Vec::new();
    for (s, packet) in input.packets() {
        if s.index() != a.stream_index {
            continue;
        }
        dec.send(&packet).unwrap();
        while let Some(c) = dec.receive().unwrap() {
            chunks.push(c);
        }
    }
    chunks
}

#[test]
fn decodes_stereo_f32_at_requested_rate() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else {
        return;
    };
    let chunks = decode_all(&path, 48_000);
    let frames: usize = chunks.iter().map(|c| c.samples.len() / 2).sum();
    assert!(
        (frames as f64 / 48_000.0 - 10.0).abs() < 0.1,
        "decoded {frames} frames"
    );
    assert!(chunks.windows(2).all(|w| w[1].pts > w[0].pts));
}

#[test]
fn resamples_44100_to_device_rate() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else {
        return;
    };
    let chunks = decode_all(&path, 48_000);
    let frames: usize = chunks.iter().map(|c| c.samples.len() / 2).sum();
    // 10 s of audio must stay 10 s after resampling (no drift)
    assert!(
        (frames as f64 / 48_000.0 - 10.0).abs() < 0.1,
        "decoded {frames} frames"
    );
    // the beep at t = 1 s must still be at 1 s
    let all: Vec<f32> = chunks
        .iter()
        .flat_map(|c| c.samples.iter().copied())
        .collect();
    let first_loud_after_half_second = all
        .chunks(2)
        .enumerate()
        .skip(24_000)
        .find(|(_, s)| s[0].abs() > 0.1)
        .map(|(i, _)| i as f64 / 48_000.0)
        .unwrap();
    eprintln!(
        "beep onset at {first_loud_after_half_second:.5} s, first chunk pts {:.5}, {frames} frames",
        chunks[0].pts
    );
    assert!(
        (first_loud_after_half_second - 1.0).abs() < 0.01,
        "beep at {first_loud_after_half_second}"
    );
}
