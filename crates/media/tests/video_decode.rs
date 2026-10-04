mod common;
use actionlay_media::{probe::probe, video::VideoDecoder};
use ffmpeg_next as ffmpeg;

fn decode_first(
    path: &std::path::Path,
    n: usize,
    prefer_hw: bool,
) -> (Vec<f64>, &'static str, (u32, u32, usize, usize)) {
    let info = probe(path).unwrap();
    let mut input = ffmpeg::format::input(path).unwrap();
    let params = input.stream(info.video.stream_index).unwrap().parameters();
    let mut dec = VideoDecoder::open(params, info.video.time_base, prefer_hw).unwrap();
    let mut pts = Vec::new();
    let mut dims = (0, 0, 0, 0);
    for (stream, packet) in input.packets() {
        if stream.index() != info.video.stream_index {
            continue;
        }
        dec.send(&packet).unwrap();
        while let Some(f) = dec.receive().unwrap() {
            dims = (f.width, f.height, f.y.len(), f.uv.len());
            pts.push(f.pts);
            if pts.len() == n {
                return (pts, dec.active_backend(), dims);
            }
        }
    }
    (pts, dec.active_backend(), dims)
}

#[test]
fn decodes_frames_with_monotonic_pts() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else {
        return;
    };
    let (pts, backend, (w, h, ylen, uvlen)) = decode_first(&path, 50, true);
    assert_eq!(pts.len(), 50);
    assert!(
        pts.windows(2).all(|p| p[1] > p[0]),
        "pts not increasing: {pts:?}"
    );
    assert!((pts[1] - pts[0] - 0.01).abs() < 1e-3);
    assert_eq!((w, h, ylen, uvlen), (1920, 1440, 1920 * 1440, 1920 * 720));
    eprintln!("backend: {backend}");
    if std::env::var("ACTIONLAY_EXPECT_HW").is_ok() {
        assert_ne!(backend, "software");
    }
}

#[test]
fn decodes_ten_bit_to_nv12() {
    let Some(path) = common::sample("hevc10-2160p60-sync.mp4") else {
        return;
    };
    let (pts, _, (w, h, ylen, _)) = decode_first(&path, 5, true);
    assert_eq!(pts.len(), 5);
    assert_eq!((w, h, ylen), (3840, 2160, 3840 * 2160));
}

#[test]
fn falls_back_to_software_when_hw_disabled() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else {
        return;
    };
    let (pts, backend, _) = decode_first(&path, 3, false);
    assert_eq!(pts.len(), 3);
    assert_eq!(backend, "software");
}

fn first_frame_mean_luma(path: &std::path::Path, prefer_hw: bool) -> f64 {
    let info = probe(path).unwrap();
    let mut input = ffmpeg::format::input(path).unwrap();
    let params = input.stream(info.video.stream_index).unwrap().parameters();
    let mut dec = VideoDecoder::open(params, info.video.time_base, prefer_hw).unwrap();
    for (stream, packet) in input.packets() {
        if stream.index() != info.video.stream_index {
            continue;
        }
        dec.send(&packet).unwrap();
        if let Some(f) = dec.receive().unwrap() {
            let sum: u64 = f.y.iter().map(|&v| u64::from(v)).sum();
            return sum as f64 / f.y.len() as f64;
        }
    }
    panic!("no frame decoded");
}

#[test]
fn full_range_white_flash_is_preserved_on_both_paths() {
    let Some(path) = common::sample("hevc8-1440p100-sync.mp4") else {
        return;
    };
    let hw = first_frame_mean_luma(&path, true);
    let sw = first_frame_mean_luma(&path, false);
    eprintln!("full range white: hw={hw} sw={sw}");
    assert!(hw >= 250.0, "hw luma {hw}");
    assert!(sw >= 250.0, "sw luma {sw}");
    assert!((hw - sw).abs() <= 2.0, "hw {hw} vs sw {sw}");
}

#[test]
fn limited_range_ten_bit_white_flash_matches_on_both_paths() {
    let Some(path) = common::sample("hevc10-2160p60-sync.mp4") else {
        return;
    };
    let hw = first_frame_mean_luma(&path, true);
    let sw = first_frame_mean_luma(&path, false);
    eprintln!("limited range white: hw={hw} sw={sw}");
    assert!((hw - 235.0).abs() <= 2.0, "hw luma {hw}");
    assert!((sw - 235.0).abs() <= 2.0, "sw luma {sw}");
    assert!((hw - sw).abs() <= 2.0, "hw {hw} vs sw {sw}");
}
