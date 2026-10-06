//! Usage: paused-seek-check FILE
//! Measures first-frame and precise seek latency without audio or playback.
use actionlay_media::player::{Player, PlayerOptions};
use std::time::{Duration, Instant};
fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: paused-seek-check FILE"))?;
    let start = Instant::now();
    let mut player = Player::open(
        path.as_ref(),
        PlayerOptions {
            audio: false,
            ..Default::default()
        },
    )?;
    println!(
        "open seconds={:.3} duration={:.3}",
        start.elapsed().as_secs_f64(),
        player.info().duration
    );
    let duration = player.info().duration;
    for (label, target) in [
        ("initial", 0.0),
        ("near-end", duration * 0.8),
        ("middle", duration * 0.5),
        ("repeat", duration * 0.8),
    ] {
        let start = Instant::now();
        if label != "initial" {
            player.seek(target, true);
        }
        loop {
            if let Some(frame) = player.poll_frame() {
                println!(
                    "{label} target={target:.3} frame={:.3} seconds={:.3} buffered={:.3}",
                    frame.pts,
                    start.elapsed().as_secs_f64(),
                    player.buffered_seconds()
                );
                anyhow::ensure!(
                    (frame.pts - target).abs() < 0.1,
                    "precise seek returned unexpected frame"
                );
                break;
            }
            anyhow::ensure!(
                start.elapsed() < Duration::from_secs(45),
                "frame request timed out"
            );
            std::thread::sleep(Duration::from_millis(4));
        }
    }
    Ok(())
}
