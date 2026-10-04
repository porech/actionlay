mod common;
use actionlay_media::player::{Player, PlayerOptions};
use std::time::{Duration, Instant};

fn wait_for_frame(p: &mut Player, timeout: Duration) -> Option<f64> {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if let Some(f) = p.poll_frame() {
            return Some(f.pts);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

/// Polls like a UI would for `dur`, returning the pts of every presented frame.
fn collect_frames(p: &mut Player, dur: Duration) -> Vec<f64> {
    let end = Instant::now() + dur;
    let mut out = Vec::new();
    while Instant::now() < end {
        if let Some(f) = p.poll_frame() {
            out.push(f.pts);
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    out
}

fn no_audio() -> PlayerOptions {
    PlayerOptions {
        prefer_hw: true,
        audio: false,
    }
}

#[test]
fn shows_first_frame_while_paused() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else {
        return;
    };
    let mut p = Player::open(&path, no_audio()).unwrap();
    assert!(p.is_paused());
    let pts = wait_for_frame(&mut p, Duration::from_secs(5)).expect("no first frame");
    assert!(pts < 0.02);
}

#[test]
fn plays_file_without_audio() {
    let Some(path) = common::sample("hevc8-1080p30-noaudio.mp4") else {
        return;
    };
    let mut p = Player::open(&path, PlayerOptions::default()).unwrap();
    assert!(!p.stats().audio_active);
    p.play();
    std::thread::sleep(Duration::from_millis(600));
    let mut last = 0.0;
    for _ in 0..50 {
        if let Some(f) = p.poll_frame() {
            last = f.pts;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(last > 0.4, "playback did not advance: {last}");
}

#[test]
fn seek_clamps_and_discards_stale_frames() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else {
        return;
    };
    let mut p = Player::open(&path, no_audio()).unwrap();
    wait_for_frame(&mut p, Duration::from_secs(5));
    // rapid seeks: only the last one must win
    p.seek(2.0, false);
    p.seek(7.5, true);
    let pts = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    assert!((pts - 7.5).abs() < 0.011, "precise seek landed at {pts}");
    p.seek(1_000.0, true);
    let pts = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    assert!(
        pts <= p.info().duration && pts > p.info().duration - 0.1,
        "clamped seek at {pts}"
    );
    p.seek(-5.0, true);
    let pts = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    assert!(pts < 0.02);
}

#[test]
fn frame_step_moves_one_frame() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else {
        return;
    };
    let mut p = Player::open(&path, no_audio()).unwrap();
    p.seek(3.0, true);
    let a = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    p.step(1);
    let b = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    p.step(-1);
    let c = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    assert!((b - a - 0.01).abs() < 1e-3, "{a} -> {b}");
    assert!((c - a).abs() < 1e-3, "{a} -> {c}");
}

// The tests below use the default options: with an output device the audio
// clock drives playback, without one the player falls back to the system
// clock. Both must behave the same from the outside. They use the 30 fps
// H.264 sample so that a debug build decoding in software keeps up.

#[test]
fn pause_and_resume_with_audio_keeps_advancing() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else {
        return;
    };
    let mut p = Player::open(&path, PlayerOptions::default()).unwrap();
    eprintln!("audio_active = {}", p.stats().audio_active);
    p.play();
    let first = collect_frames(&mut p, Duration::from_millis(700));
    p.pause();
    let paused_at = p.position();
    assert!(collect_frames(&mut p, Duration::from_millis(300)).is_empty());
    assert_eq!(p.position(), paused_at, "position moved while paused");
    // resuming re-seeks to the pause position (fresh audio), then plays on
    p.play();
    let resumed = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    assert!(
        (resumed - paused_at).abs() < 0.034,
        "resumed at {resumed}, paused at {paused_at}"
    );
    let mut second = vec![resumed];
    second.extend(collect_frames(&mut p, Duration::from_millis(700)));
    let all: Vec<f64> = first.iter().chain(&second).copied().collect();
    assert!(
        all.windows(2).all(|w| w[1] >= w[0]),
        "pts went backwards: {all:?}"
    );
    let (a, b) = (*first.last().unwrap(), *second.last().unwrap());
    assert!(a > 0.4, "first run did not advance: {a}");
    assert!(
        b > resumed + 0.4,
        "resume did not advance: {resumed} -> {b}"
    );
}

#[test]
fn speed_change_with_audio_keeps_advancing() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else {
        return;
    };
    let mut p = Player::open(&path, PlayerOptions::default()).unwrap();
    p.play();
    wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    collect_frames(&mut p, Duration::from_millis(300));
    p.set_speed(2.0);
    let start = p.position();
    let fast = collect_frames(&mut p, Duration::from_millis(1000));
    let after_fast = *fast.last().unwrap();
    assert!(
        after_fast - start > 1.2,
        "2x did not run at 2x: {start} -> {after_fast}"
    );
    // back to 1x: audio drives again after a re-seek
    p.set_speed(1.0);
    let resumed = wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    let normal = collect_frames(&mut p, Duration::from_millis(700));
    let end = *normal.last().unwrap();
    assert!(
        end - resumed > 0.4 && end - resumed < 1.0,
        "1x after 2x: {resumed} -> {end}"
    );
}

#[test]
fn plays_to_the_end_and_pauses() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else {
        return;
    };
    let mut p = Player::open(&path, PlayerOptions::default()).unwrap();
    p.seek(9.5, true);
    wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    p.play();
    wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    let frames = collect_frames(&mut p, Duration::from_millis(1000));
    assert!(p.at_end(), "not at end, last {:?}", frames.last());
    assert!(p.is_paused());
    assert!(*frames.last().unwrap() > 9.9);
}

#[test]
fn drop_does_not_hang() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else {
        return;
    };
    for play in [false, true] {
        let mut p = Player::open(&path, PlayerOptions::default()).unwrap();
        if play {
            p.play();
        }
        // let the decode thread fill every buffer, then never poll again
        std::thread::sleep(Duration::from_millis(500));
        let t = Instant::now();
        drop(p);
        assert!(
            t.elapsed() < Duration::from_secs(2),
            "drop took {:?}",
            t.elapsed()
        );
    }
}
