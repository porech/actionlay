mod common;
use actionlay_media::gpmf::read_gpmf_packets;

#[test]
fn reads_hero5_metadata_packets() {
    let Some(path) = common::gopro_sample("hero5.mp4") else {
        return;
    };
    let packets = read_gpmf_packets(&path).unwrap();
    assert_eq!(packets.len(), 34);
    assert_eq!(packets[0].pts, 0.0);
    assert!((packets[1].pts - 1.001).abs() < 1e-9);
    assert!((packets[33].pts - 33.033).abs() < 1e-9);
    assert!(packets.iter().all(|p| (p.duration - 1.001).abs() < 1e-9));
    assert_eq!(packets[0].data.len(), 4792);
    assert_eq!(&packets[0].data[..4], b"DEVC");
}

#[test]
fn keeps_the_short_final_packet_duration() {
    let Some(path) = common::gopro_sample("max-heromode.mp4") else {
        return;
    };
    let packets = read_gpmf_packets(&path).unwrap();
    assert_eq!(packets.len(), 11);
    let last = packets.last().unwrap();
    assert!((last.pts - 10.01).abs() < 1e-9);
    assert!((last.duration - 0.533).abs() < 1e-9);
}

#[test]
fn file_without_metadata_has_no_packets() {
    let Some(path) = common::sample("hevc8-1080p30-noaudio.mp4") else {
        return;
    };
    assert!(read_gpmf_packets(&path).unwrap().is_empty());
}

#[test]
fn missing_file_is_an_error() {
    assert!(read_gpmf_packets(std::path::Path::new("/nonexistent/video.mp4")).is_err());
}

/// Temp directory removed on drop, so cleanup happens even when an
/// assertion fails.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("actionlay-gpmf-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn truncated_file_without_moov_cannot_be_opened() {
    let Some(path) = common::gopro_sample("hero5.mp4") else {
        return;
    };
    // hero5.mp4 has its moov atom at the end: half of it is unreadable.
    let bytes = std::fs::read(&path).unwrap();
    let tmp = TempDir::new("nomoov");
    let cut = tmp.0.join("cut.mp4");
    std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    assert!(read_gpmf_packets(&cut).is_err());
}

#[test]
fn truncated_faststart_file_returns_partial_packets() {
    let Some(path) = common::gopro_sample("hero5.mp4") else {
        return;
    };
    let tmp = TempDir::new("faststart");
    let fast = tmp.0.join("fast.mp4");
    let remuxed = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i"])
        .arg(&path)
        // Only video, audio and gpmd: the tmcd stream cannot be remuxed.
        .args(["-map", "0:0", "-map", "0:1", "-map", "0:3"])
        .args(["-c", "copy", "-movflags", "+faststart"])
        .arg(&fast)
        .status();
    let Ok(status) = remuxed else {
        eprintln!("ffmpeg CLI not available, skipping");
        return;
    };
    assert!(status.success(), "ffmpeg remux failed");
    let full = read_gpmf_packets(&fast).unwrap().len();
    let bytes = std::fs::read(&fast).unwrap();
    let cut = tmp.0.join("cut.mp4");
    std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    // The moov is at the front, so the file opens and the short mdat gives
    // a partial list. The log::warn arm itself stays untested: a short mdat
    // usually ends in EOF rather than in a read error.
    let n = read_gpmf_packets(&cut).unwrap().len();
    assert!(n > 0 && n < full, "{n} packets of {full}");
}
