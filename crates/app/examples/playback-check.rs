//! A limited playback/seek check using the same player and telemetry loader as UI.
//! Usage: playback-check FILE [SEEK_SECONDS]
#[path = "../src/telemetry_load.rs"]
mod telemetry_load;
use actionlay_media::player::{Player, PlayerOptions};
use actionlay_telemetry::{Metric, Value};
use std::time::{Duration, Instant};
fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: playback-check FILE [SEEK_SECONDS]"))?;
    let seek = std::env::args().nth(2).and_then(|s| s.parse::<f64>().ok());
    let start = Instant::now();
    let mut p = Player::open(path.as_ref(), PlayerOptions::default())?;
    let meta = p.info().telemetry.clone();
    let loader = meta.and_then(|m| {
        p.take_telemetry()
            .map(|rx| telemetry_load::spawn(rx, p.info().duration, m.packet_count, || {}))
    });
    eprintln!(
        "opened in {:.3}s, {}x{}, {:.1}fps, {:.1}s duration",
        start.elapsed().as_secs_f64(),
        p.info().video.width,
        p.info().video.height,
        p.info().video.fps,
        p.info().duration
    );
    let mut telemetry = None;
    for target in [None, seek, Some(0.0)] {
        if let Some(t) = target {
            p.seek(t, true);
            eprintln!("seek to {t:.3}s");
        }
        p.play();
        let start = Instant::now();
        let mut last = Instant::now();
        let mut previous = p.stats().presented;
        let mut buffered = Duration::ZERO;
        let mut polls = 0;
        let mut max_av: f64 = 0.0;
        while start.elapsed() < Duration::from_secs(10) {
            let tick = Instant::now();
            let frame = p.poll_frame();
            if let Some(update) = loader.as_ref().and_then(|l| l.take_update()) {
                eprintln!(
                    "telemetry update after {:.3}s, metadata through {:.3}s",
                    start.elapsed().as_secs_f64(),
                    update.telemetry.duration()
                );
                if let Some(warning) = update.warning {
                    eprintln!("{warning}");
                }
                telemetry = Some(update.telemetry);
            }
            if let Some(offset) = p.stats().av_offset
                && frame.is_some()
            {
                max_av = max_av.max(offset.abs());
            }
            if last.elapsed() >= Duration::from_secs(1) {
                let st = p.stats();
                eprintln!(
                    "t={:.3}s frames/s={} buffered={:.2}s buffering={} speed-data={}",
                    p.position(),
                    st.presented - previous,
                    p.buffered_seconds(),
                    p.is_buffering(),
                    telemetry.as_ref().is_some_and(|t| matches!(
                        t.sample(p.position()).get(Metric::Speed),
                        Value::Present(_)
                    ))
                );
                previous = st.presented;
                last = Instant::now();
            }
            polls += 1;
            std::thread::sleep(Duration::from_millis(4));
            if p.is_buffering() {
                buffered += tick.elapsed();
            }
        }
        eprintln!(
            "stage: buffering {:.3}s, max A/V {:.3}s, {polls} polls",
            buffered.as_secs_f64(),
            max_av
        );
        p.pause();
    }
    Ok(())
}
