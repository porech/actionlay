mod common;
use actionlay_media::{
    chapters::{ChapterPlayer, Timeline, discover, is_first_chapter},
    player::PlayerOptions,
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "actionlay-chapters-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn touch(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, []).unwrap();
    p
}
#[test]
fn discovery_is_ordered_conservative_and_handles_legacy_names() {
    let d = Temp::new();
    let a = touch(&d.0, "GX014821.MP4");
    let b = touch(&d.0, "GX024821.MP4");
    touch(&d.0, "GX014822.MP4");
    assert_eq!(discover(&b), vec![a.clone(), b.clone()]);
    assert!(is_first_chapter(&a));
    assert!(!is_first_chapter(&b));
    let c = touch(&d.0, "GX044821.MP4");
    assert_eq!(discover(&a), vec![a.clone()]);
    std::fs::remove_file(c).unwrap();
    touch(&d.0, "gx014821.mp4"); // duplicate chapter is ambiguous
    if std::fs::read_dir(&d.0).unwrap().count() > 3 {
        assert_eq!(discover(&a), vec![a]);
    }
    let a = touch(&d.0, "GOPR1234.MP4");
    let b = touch(&d.0, "G0011234.MP4");
    let c = touch(&d.0, "G0021234.MP4");
    assert!(is_first_chapter(&a));
    assert!(!is_first_chapter(&b));
    assert_eq!(discover(&b), vec![a, b, c]);
    let plain = touch(&d.0, "holiday.mp4");
    assert_eq!(discover(&plain), vec![plain]);
}
fn next(p: &mut ChapterPlayer) -> actionlay_media::frame::Nv12Frame {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(f) = p.poll_frame() {
            return f;
        }
        assert!(
            Instant::now() < deadline,
            "no chapter frame: {:?}",
            p.take_error()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn seek_step_and_automatic_playback_cross_chapter_boundaries() {
    let Some(sample) = common::sample("hevc8-1080p30-noaudio.mp4") else {
        return;
    };
    let d = Temp::new();
    let a = d.0.join("GX010001.MP4");
    let b = d.0.join("GX020001.MP4");
    std::fs::hard_link(&sample, &a).unwrap();
    std::fs::hard_link(&sample, &b).unwrap();
    let timeline = Timeline::open(&a, true).unwrap();
    assert_eq!(timeline.chapters.len(), 2);
    let boundary = timeline.chapters[1].start;
    assert_eq!(timeline.locate(boundary), (1, 0.0));
    let opts = PlayerOptions {
        audio: false,
        prefer_hw: false,
        ..Default::default()
    };
    let intermediate = ChapterPlayer::open_with_audio_device(&b, opts, None).unwrap();
    assert_eq!(intermediate.timeline().chapters.len(), 1);
    assert_eq!(intermediate.timeline().chapters[0].path, b);
    drop(intermediate);
    let sequence = ChapterPlayer::open_mode(&b, opts, None, true).unwrap();
    assert_eq!(sequence.timeline().chapters.len(), 2);
    assert_eq!(sequence.timeline().chapters[0].path, a);
    drop(sequence);
    let mut p = ChapterPlayer::open_with_audio_device(&a, opts, None).unwrap();
    next(&mut p);
    p.seek(boundary + 0.5, true);
    let f = next(&mut p);
    assert!((f.pts - (boundary + 0.5)).abs() < 0.06, "{}", f.pts);
    p.seek(boundary - 1.0 / 30.0, true);
    next(&mut p);
    p.step(2);
    let f = next(&mut p);
    assert!(f.pts >= boundary);
    p.seek(boundary - 0.1, true);
    next(&mut p);
    p.play();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(f) = p.poll_frame()
            && f.pts >= boundary
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "playback did not advance: {:?}",
            p.take_error()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!p.is_paused());
    let alone = ChapterPlayer::open_mode(&b, opts, None, false).unwrap();
    assert_eq!(alone.timeline().chapters.len(), 1);
    assert!(alone.info().duration < timeline.duration);
}
