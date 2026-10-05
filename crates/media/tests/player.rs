mod common;
use actionlay_media::player::{Player, PlayerOptions};
use std::time::{Duration, Instant};

// Route audible integration checks to a virtual output when requested.
fn open(
    path: &std::path::Path,
    options: PlayerOptions,
) -> Result<Player, actionlay_media::MediaError> {
    let device = std::env::var("ACTIONLAY_TEST_AUDIO_DEVICE").ok();
    Player::open_with_audio_device(path, options, device.as_deref())
}

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
    let mut p = open(&path, no_audio()).unwrap();
    assert!(p.is_paused());
    let pts = wait_for_frame(&mut p, Duration::from_secs(5)).expect("no first frame");
    assert!(pts < 0.02);
}

#[test]
fn plays_file_without_audio() {
    let Some(path) = common::sample("hevc8-1080p30-noaudio.mp4") else {
        return;
    };
    let mut p = open(&path, PlayerOptions::default()).unwrap();
    assert!(!p.stats().audio_active);
    p.play();
    wait_for_frame(&mut p, Duration::from_secs(5)).expect("no preview frame");
    let frames = collect_frames(&mut p, Duration::from_millis(700));
    let last = last(&frames, "buffered playback");
    assert!(last > 0.4, "playback did not advance: {last}");
}

#[test]
fn seek_clamps_and_discards_stale_frames() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else {
        return;
    };
    let mut p = open(&path, no_audio()).unwrap();
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
    let mut p = open(&path, no_audio()).unwrap();
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

fn last(frames: &[f64], what: &str) -> f64 {
    *frames
        .last()
        .unwrap_or_else(|| panic!("no frame presented during {what}"))
}

#[test]
fn pause_and_resume_with_audio_keeps_advancing() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else {
        return;
    };
    let mut p = open(&path, PlayerOptions::default()).unwrap();
    eprintln!("audio_active = {}", p.stats().audio_active);
    p.play();
    let first = collect_frames(&mut p, Duration::from_millis(700));
    p.pause();
    let paused_at = p.position();
    assert!(collect_frames(&mut p, Duration::from_millis(300)).is_empty());
    assert_eq!(p.position(), paused_at, "position moved while paused");
    // resuming re-seeks to the pause position (fresh audio), then plays on
    p.play();
    let resumed = wait_for_frame(&mut p, Duration::from_secs(5)).expect("no frame after resume");
    assert!(
        (resumed - paused_at).abs() < 0.1,
        "resumed at {resumed}, paused at {paused_at}"
    );
    let mut second = vec![resumed];
    second.extend(collect_frames(&mut p, Duration::from_millis(700)));
    let all: Vec<f64> = first.iter().chain(&second).copied().collect();
    assert!(
        all.windows(2).all(|w| w[1] >= w[0]),
        "pts went backwards: {all:?}"
    );
    let (a, b) = (last(&first, "first run"), last(&second, "resumed run"));
    assert!(a > 0.2, "first run did not advance: {a}");
    assert!(
        b > resumed + 0.2,
        "resume did not advance: {resumed} -> {b}"
    );
}

#[test]
fn speed_change_with_audio_keeps_advancing() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else {
        return;
    };
    let mut p = open(&path, PlayerOptions::default()).unwrap();
    p.play();
    let start = wait_for_frame(&mut p, Duration::from_secs(5)).expect("no first frame");
    let normal = collect_frames(&mut p, Duration::from_millis(1000));
    let at_1x = last(&normal, "1x") - start;
    p.set_speed(2.0);
    let from = p.position();
    let fast = collect_frames(&mut p, Duration::from_millis(1000));
    let at_2x = last(&fast, "2x") - from;
    assert!(at_1x > 0.3, "1x did not advance: {at_1x}");
    assert!(
        at_2x > at_1x + 0.2,
        "2x not faster than 1x: {at_2x} vs {at_1x} in 1 s"
    );
    // back to 1x: audio drives again after a re-seek
    p.set_speed(1.0);
    let resumed = wait_for_frame(&mut p, Duration::from_secs(5)).expect("no frame back at 1x");
    let again = collect_frames(&mut p, Duration::from_millis(700));
    let advanced = last(&again, "1x after 2x") - resumed;
    assert!(
        advanced > 0.2 && advanced < 1.0,
        "1x after 2x advanced {advanced} in 0.7 s"
    );
}

#[test]
fn plays_to_the_end_and_pauses() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else {
        return;
    };
    let mut p = open(&path, PlayerOptions::default()).unwrap();
    p.seek(9.5, true);
    wait_for_frame(&mut p, Duration::from_secs(5)).expect("no frame after seek");
    p.play();
    let mut frames =
        vec![wait_for_frame(&mut p, Duration::from_secs(5)).expect("no frame after play")];
    let deadline = Instant::now() + Duration::from_secs(5);
    while !p.at_end() && Instant::now() < deadline {
        frames.extend(p.poll_frame().map(|f| f.pts));
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(p.at_end(), "not at end, last {:?}", frames.last());
    assert!(p.is_paused());
    let end = last(&frames, "the run to the end");
    assert!(end > 9.9, "last frame {end}");
}

#[test]
fn drop_does_not_hang() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else {
        return;
    };
    for play in [false, true] {
        let mut p = open(&path, PlayerOptions::default()).unwrap();
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

#[test]
fn suspended_presentation_resumes_at_the_clock_without_replaying_backlog() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else {
        return;
    };
    for options in [no_audio(), PlayerOptions::default()] {
        let mut p = open(&path, options).unwrap();
        p.play();
        wait_for_frame(&mut p, Duration::from_secs(5)).expect("no initial frame");
        collect_frames(&mut p, Duration::from_millis(300));
        // Simulate a UI which stops drawing while the playback clock runs.
        std::thread::sleep(Duration::from_millis(1400));
        assert!(p.poll_frame().is_none(), "presented an obsolete frame");
        let target = p.position();
        assert!(target > 1.0, "clock did not advance: {target}");
        let resumed = wait_for_frame(&mut p, Duration::from_secs(5)).expect("no resync frame");
        assert!(
            (resumed - target).abs() < 0.1,
            "replayed backlog: {resumed} vs {target}"
        );
        let normal = collect_frames(&mut p, Duration::from_millis(500));
        let advanced = last(&normal, "resynchronized playback") - resumed;
        assert!(
            advanced > 0.15 && advanced < 0.9,
            "catch-up playback: {advanced}"
        );
    }
}

#[test]
fn metadata_streams_from_the_player_while_paused_and_after_seek() {
    let Some(path) = common::gopro_sample("hero5.mp4") else {
        return;
    };
    let mut p = open(&path, no_audio()).unwrap();
    assert!(p.info().telemetry.is_some());
    let rx = p.take_telemetry().unwrap();
    wait_for_frame(&mut p, Duration::from_secs(5)).expect("no preview");
    let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let actionlay_media::player::TelemetryEvent::Packet { packet, .. } = first else {
        panic!("no metadata packet")
    };
    assert!(packet.pts < 1.1);
    assert!(!packet.data.is_empty());
    std::thread::sleep(Duration::from_millis(100));
    assert!(rx.try_iter().all(|event|matches!(event,actionlay_media::player::TelemetryEvent::Packet {packet,..} if packet.pts<5.0)),"read metadata from the whole file");
    p.seek(15.0, true);
    assert!(p.is_paused());
    assert!((wait_for_frame(&mut p, Duration::from_secs(5)).unwrap() - 15.0).abs() < 0.1);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut found = false;
    while Instant::now() < deadline {
        if let Ok(actionlay_media::player::TelemetryEvent::Packet { packet, .. }) =
            rx.recv_timeout(Duration::from_millis(50))
            && (12.0..18.0).contains(&packet.pts)
        {
            found = true;
            break;
        }
    }
    assert!(found, "seek did not deliver metadata from its new position");
}

#[test]
fn switching_output_preserves_time_speed_pause_and_metadata_receiver() {
    let Some(path) = common::sample("h264-1080p30-44k.mp4") else {
        return;
    };
    let mut p = open(&path, PlayerOptions::default()).unwrap();
    if !p.stats().audio_active {
        return;
    } // Headless CI has no output device.
    let receiver = p.take_telemetry().unwrap();
    p.seek(2.0, true);
    wait_for_frame(&mut p, Duration::from_secs(5)).unwrap();
    let device = std::env::var("ACTIONLAY_TEST_AUDIO_DEVICE").ok();
    p.set_speed(1.5);
    let at = p.position();
    assert!(p.change_audio_device(device.as_deref()).unwrap());
    assert!(p.is_paused());
    assert_eq!(p.speed(), 1.5);
    assert!((wait_for_frame(&mut p, Duration::from_secs(5)).unwrap() - at).abs() < 0.05);
    assert!(
        p.take_telemetry().is_none(),
        "metadata receiver was replaced"
    );
    assert!(!matches!(
        receiver.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
    p.set_speed(1.0);
    p.play();
    collect_frames(&mut p, Duration::from_millis(300));
    let at = p.position();
    assert!(p.change_audio_device(device.as_deref()).unwrap());
    assert!(!p.is_paused());
    let frames = collect_frames(&mut p, Duration::from_millis(700));
    assert!(last(&frames, "changed device") > at + 0.2);
    assert!(
        p.change_audio_device(Some("__missing_ActionLay_output__"))
            .is_err()
    );
    assert!(!p.is_paused(), "failed switch interrupted playback");
}
