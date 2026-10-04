//! Usage: decode-bench <video> [--sw]
//! Decodes the whole video stream to NV12 and reports frames per second.
use std::time::Instant;

use actionlay_media::{probe::probe, video::VideoDecoder};
use ffmpeg_next as ffmpeg;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: decode-bench <video> [--sw]")?;
    let prefer_hw = args.next().as_deref() != Some("--sw");
    let info = probe(path.as_ref())?;
    let mut input = ffmpeg::format::input(&path)?;
    let params = input
        .stream(info.video.stream_index)
        .ok_or("no video")?
        .parameters();
    let mut dec = VideoDecoder::open(params, info.video.time_base, prefer_hw)?;

    let start = Instant::now();
    let mut frames = 0u64;
    for (stream, packet) in input.packets() {
        if stream.index() != info.video.stream_index {
            continue;
        }
        dec.send(&packet)?;
        while dec.receive()?.is_some() {
            frames += 1;
        }
    }
    dec.send_eof()?;
    while dec.receive()?.is_some() {
        frames += 1;
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "{path}: {}x{} @ {:.2} fps source, backend={}, {frames} frames in {secs:.2}s = {:.1} fps decoded",
        info.video.width,
        info.video.height,
        info.video.fps,
        dec.active_backend(),
        frames as f64 / secs
    );
    Ok(())
}
