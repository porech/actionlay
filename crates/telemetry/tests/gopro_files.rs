//! Behaviour on the public GoPro samples: gaps, no lock, short packets,
//! damaged packets.
mod common;

use actionlay_telemetry::{GpsLock, Metric, Telemetry, TelemetryOptions, Value};

#[test]
fn hero5_locked_throughout() {
    let Some(tel) = common::load("hero5.mp4") else {
        return;
    };
    assert!((tel.duration() - 34.034).abs() < 1e-9);
    assert_eq!(tel.gps_points().len(), 618);
    assert_eq!(tel.track().len(), 618);
    // median of GPSU − t over the 618 locked points; the first packet
    // alone said 17:31:03.000
    assert_eq!(
        tel.start_utc().unwrap().to_rfc3339(),
        "2017-04-17T17:31:02.978+00:00"
    );
    let a = tel.availability();
    for m in [
        Metric::Lat,
        Metric::Speed,
        Metric::CSpeed,
        Metric::AcclZ,
        Metric::Temp,
    ] {
        assert_eq!(a.coverage(m), 1.0, "{}", m.id());
    }
    // accel needs a 3 s window
    assert_eq!(a.gaps(Metric::Accel).len(), 1);
    assert!((a.gaps(Metric::Accel)[0].1 - 3.003).abs() < 1e-9);
    assert!(!a.is_available(Metric::GravX));
    let snap = tel.sample(10.0);
    assert_eq!(snap.gps_lock, GpsLock::Lock3d);
    assert!((snap.get(Metric::AcclZ).present().unwrap() - 9.8).abs() < 1.5);
    assert!(tel.warnings().is_empty());
}

#[test]
fn hero5_video_duration_covers_the_tail() {
    let Some(path) = common::gopro_sample("hero5.mp4") else {
        return;
    };
    let packets = common::raw_packets(&path);
    let plain = Telemetry::from_gpmf_packets(&packets).unwrap();
    assert!(matches!(
        plain.sample(34.5).get(Metric::Speed),
        Value::Stale { .. }
    ));
    // the video track is 34.576 s, the metadata track 34.034 s
    let opts = TelemetryOptions {
        video_duration: Some(34.576),
        ..TelemetryOptions::default()
    };
    let tel = Telemetry::from_gpmf_packets_with(&packets, &opts).unwrap();
    assert!((tel.duration() - 34.034).abs() < 1e-9);
    for m in [Metric::Speed, Metric::Lat, Metric::AcclZ, Metric::Temp] {
        assert!(tel.sample(34.5).get(m).present().is_some(), "{}", m.id());
    }
    let a = tel.availability();
    assert!((a.coverage(Metric::Lat) - 1.0).abs() < 1e-9);
    assert!(a.gaps(Metric::Lat).is_empty());
    assert!((a.gaps(Metric::Accel)[0].1 - 3.003).abs() < 1e-9);
    assert!((a.coverage(Metric::Accel) - (34.576 - 3.003) / 34.576).abs() < 1e-9);
}

#[test]
fn hero6_fix_gap_is_stale_then_recovers() {
    let Some(tel) = common::load("hero6.mp4") else {
        return;
    };
    // packet 0 is locked, packets 1–13 have no fix, 2D from packet 14
    let lat = tel.sample(5.0).get(Metric::Lat);
    let Value::Stale { age, .. } = lat else {
        panic!("{lat:?}")
    };
    assert!((age - 3.999).abs() < 1e-6, "age {age}");
    assert_eq!(tel.sample(5.0).gps_lock, GpsLock::NoLock);
    assert_eq!(tel.sample(14.5).gps_lock, GpsLock::Lock2d);
    assert_eq!(tel.sample(20.0).gps_lock, GpsLock::Lock3d);
    assert!(tel.sample(20.0).get(Metric::Lat).present().is_some());
    let a = tel.availability();
    let gaps = a.gaps(Metric::Lat);
    assert_eq!(gaps.len(), 1);
    assert!((gaps[0].0 - 1.001).abs() < 1e-9 && (gaps[0].1 - 14.014).abs() < 1e-9);
    assert!((a.coverage(Metric::Lat) - 10.01 / 23.023).abs() < 1e-9);
    assert_eq!(a.coverage(Metric::GpsDop), 1.0);
}

#[test]
fn hero7_and_hero8_never_lock() {
    for (name, start) in [
        ("hero7.mp4", "2019-11-18T23:42:08.755+00:00"),
        ("hero8.mp4", "2019-11-18T23:42:08.645+00:00"),
    ] {
        let Some(tel) = common::load(name) else {
            continue;
        };
        for t in [0.0, 3.3, 9.9] {
            let snap = tel.sample(t);
            assert_eq!(snap.get(Metric::Lat), Value::Absent, "{name} {t}");
            assert_eq!(snap.get(Metric::Speed), Value::Absent, "{name} {t}");
            assert_eq!(
                snap.get(Metric::GpsDop),
                Value::Present(99.99),
                "{name} {t}"
            );
            assert_eq!(snap.get(Metric::GpsLock), Value::Present(0.0), "{name} {t}");
            assert_eq!(snap.gps_lock, GpsLock::NoLock, "{name} {t}");
            assert!(snap.get(Metric::AcclZ).present().is_some(), "{name} {t}");
        }
        let a = tel.availability();
        assert_eq!(a.coverage(Metric::Lat), 0.0, "{name}");
        assert_eq!(a.coverage(Metric::CSpeed), 0.0, "{name}");
        assert!(tel.track().is_empty(), "{name}");
        // the date still comes from the receiver's clock
        assert_eq!(tel.start_utc().unwrap().to_rfc3339(), start, "{name}");
    }
    if let Some(tel) = common::load("hero8.mp4") {
        let a = tel.availability();
        assert!(a.is_available(Metric::GravZ) && a.is_available(Metric::OriYaw));
    }
}

#[test]
fn max_short_final_packet_stays_inside_the_file() {
    let Some(tel) = common::load("max-heromode.mp4") else {
        return;
    };
    let pts = tel.gps_points();
    assert!(pts.windows(2).all(|w| w[0].t < w[1].t));
    assert!(pts.iter().all(|p| p.end <= tel.duration() + 1e-9));
    let last: Vec<_> = pts.iter().filter(|p| p.packet == 10).collect();
    assert_eq!(last.len(), 10);
    assert!((last[0].t - 10.01).abs() < 1e-9);
    assert!((last[9].end - 10.543).abs() < 1e-9);
    let a = tel.availability();
    for m in [Metric::GravX, Metric::OriPitch, Metric::Lat] {
        assert_eq!(a.coverage(m), 1.0, "{}", m.id());
    }
}

#[test]
fn max_start_utc_ignores_the_gpsu_step() {
    let Some(tel) = common::load("max-heromode.mp4") else {
        return;
    };
    // GPSU − t (dump --points): packet 0 says 23:45:14.000; packets 1–10
    // say 23:45:15.392–15.469 (GPSU jumps +2.47 s after packet 0, then
    // tracks file time). The median over the 191 locked points is packet
    // 5's 23:45:15.425; the first packet would be 1.425 s off.
    assert_eq!(
        tel.start_utc().unwrap().to_rfc3339(),
        "2019-11-18T23:45:15.425+00:00"
    );
    let utc = tel.sample(5.005).utc.unwrap();
    assert_eq!(utc.to_rfc3339(), "2019-11-18T23:45:20.430+00:00");
}

#[test]
fn damaged_packets_are_skipped() {
    let Some(path) = common::gopro_sample("hero5.mp4") else {
        return;
    };
    let packets = common::raw_packets(&path);
    let full = Telemetry::from_gpmf_packets(&packets).unwrap();
    let in_packet_3 = full.gps_points().iter().filter(|p| p.packet == 3).count();
    for cut in (1..packets[3].data.len()).step_by(97) {
        let mut damaged = packets.clone();
        damaged[3].data.truncate(cut);
        let tel = Telemetry::from_gpmf_packets(&damaged).unwrap();
        assert_eq!(tel.warnings().len(), 1, "cut {cut}");
        assert_eq!(tel.gps_points().len(), 618 - in_packet_3, "cut {cut}");
        assert!(
            tel.sample(3.5).get(Metric::Lat).present().is_some(),
            "cut {cut}"
        );
    }
    // flipped bytes: never a panic
    for at in (0..packets[3].data.len()).step_by(13) {
        let mut damaged = packets.clone();
        damaged[3].data[at] ^= 0xff;
        let _ = Telemetry::from_gpmf_packets(&damaged).unwrap();
    }
}
