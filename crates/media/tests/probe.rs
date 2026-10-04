mod common;
use actionlay_media::{
    color::{Matrix, Range},
    probe::probe,
};

#[test]
fn probes_gopro_like_sample() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else {
        return;
    };
    let info = probe(&path).unwrap();
    assert_eq!(info.video.codec, "hevc");
    assert_eq!((info.video.width, info.video.height), (1920, 1440));
    assert!(
        (info.video.fps - 100.0).abs() < 0.01,
        "fps {}",
        info.video.fps
    );
    assert_eq!(info.video.color.range, Range::Full);
    assert_eq!(info.video.color.matrix, Matrix::Bt709);
    assert!(!info.video.ten_bit);
    assert_eq!(info.audio.as_ref().unwrap().sample_rate, 48_000);
    assert!((info.duration - 10.0).abs() < 0.1);
}

#[test]
fn probes_ten_bit_and_missing_audio() {
    if let Some(path) = common::sample("hevc10-2160p60-sync.mp4") {
        let info = probe(&path).unwrap();
        assert!(info.video.ten_bit);
        assert_eq!(info.video.color.range, Range::Limited);
    }
    if let Some(path) = common::sample("hevc8-1080p30-noaudio.mp4") {
        assert!(probe(&path).unwrap().audio.is_none());
    }
}

#[test]
fn probe_reports_missing_file() {
    assert!(probe(std::path::Path::new("/nonexistent/video.mp4")).is_err());
}
