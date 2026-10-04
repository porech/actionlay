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
