use actionlay_telemetry::{
    Metric, Telemetry, Value,
    external::{Activity, ExternalError},
};
use std::io::Cursor;

fn activity() -> Activity {
    Activity::from_gpx(Cursor::new(include_str!("fixtures/activity.gpx"))).unwrap()
}

#[test]
fn gpx_extensions_timezones_segments_and_gaps() {
    let a = activity();
    assert_eq!(a.points.len(), 4);
    assert_eq!(a.points[0].values[&Metric::Power], 200.0);
    assert_eq!(a.points[0].values[&Metric::Cadence], 80.0);
    assert_eq!(a.points[0].values[&Metric::Temp], 21.0);
    let tel = a.standalone();
    assert_eq!(tel.sample(0.5).get(Metric::Hr), Value::Present(130.0));
    assert!(matches!(
        tel.sample(4.0).get(Metric::Hr),
        Value::Stale { .. }
    ));
    assert_eq!(tel.availability().gaps(Metric::Hr), &[(2.0, 8.0)]);
    assert_eq!(tel.sample(8.5).get(Metric::Hr), Value::Present(160.0)); // no cross-segment interpolation
    assert_eq!(
        tel.sample(9.0).get(Metric::Alt),
        Value::Stale {
            value: 100.0,
            age: 7.0
        }
    );
    assert!(tel.sample(9.0).get(Metric::Speed).present().is_none());
    assert!(tel.sample(1.0).get(Metric::Speed).present().unwrap() > 1.0);
    assert_eq!(tel.track().len(), 4);
    assert!(!tel.has_imu_acceleration());
}

#[test]
fn alignment_clips_and_moves_data_and_requires_known_utc() {
    let a = activity();
    let origin = a.points[0].utc;
    assert!(matches!(
        a.align(None, 10.0, 0.0),
        Err(ExternalError::MissingVideoTime)
    ));
    assert!(matches!(
        a.align(Some(origin), 10.0, f64::NAN),
        Err(ExternalError::InvalidSync)
    ));
    assert!(matches!(
        a.align(Some(origin), 10.0, 100.0),
        Err(ExternalError::NoOverlap)
    ));
    let tel = a.align(Some(origin), 10.0, 2.0).unwrap();
    assert_eq!(tel.sample(1.0).get(Metric::Hr), Value::Absent);
    assert_eq!(tel.sample(2.5).get(Metric::Hr), Value::Present(130.0));
    assert_eq!(tel.sample(10.0).get(Metric::Hr), Value::Absent);
    let tel = a.align(Some(origin), 5.0, -1.0).unwrap();
    assert_eq!(tel.sample(0.0).get(Metric::Hr), Value::Present(140.0));
}

#[test]
fn external_precedence_camera_fallback_and_union_coverage() {
    let a = activity();
    let camera = Telemetry::preview();
    let ext = a.align(camera.start_utc(), 60.0, 0.0).unwrap();
    let merged = camera.merge_external(&ext, 60.0);
    assert_eq!(merged.sample(0.5).get(Metric::Hr), Value::Present(130.0));
    assert_eq!(
        merged.sample(4.0).get(Metric::Hr),
        camera.sample(4.0).get(Metric::Hr)
    );
    assert_eq!(
        merged.sample(4.0).get(Metric::AcclX),
        camera.sample(4.0).get(Metric::AcclX)
    );
    assert_eq!(merged.availability().coverage(Metric::Hr), 1.0);
    assert_ne!(merged.identity(), camera.identity());
}

#[test]
fn malformed_missing_times_and_bad_coordinates_are_not_invented() {
    assert!(Activity::from_gpx(Cursor::new("<gpx><trk>")).is_err());
    assert!(Activity::from_gpx(Cursor::new("<gpx><trkpt lat=\"0\" lon=\"0\"/></gpx>")).is_err());
    let xml = "<gpx><trk><trkseg><trkpt lat=\"NaN\" lon=\"0\"><time>2025-01-01T00:00:00Z</time><extensions><hr>123</hr></extensions></trkpt></trkseg></trk></gpx>";
    let a = Activity::from_gpx(Cursor::new(xml)).unwrap();
    let tel = a.standalone();
    assert_eq!(tel.sample(0.0).get(Metric::Hr), Value::Present(123.0));
    assert_eq!(tel.sample(0.0).get(Metric::Lat), Value::Absent);
    assert!(tel.track().is_empty());
}

fn crc(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in bytes {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xa001
            } else {
                crc >> 1
            };
        }
    }
    crc
}
fn fit() -> Vec<u8> {
    // A little-endian FIT record definition, then two one-second records.
    let fields = [
        (253, 4, 0x86),
        (0, 4, 0x85),
        (1, 4, 0x85),
        (2, 2, 0x84),
        (3, 1, 2),
        (4, 1, 2),
        (6, 2, 0x84),
        (7, 2, 0x84),
        (13, 1, 1),
        (73, 4, 0x86),
    ];
    let mut data = vec![0x40, 0, 0, 20, 0, fields.len() as u8];
    for (n, size, kind) in fields {
        data.extend([n, size, kind]);
    }
    for i in 0..2u32 {
        data.push(0);
        data.extend((1_000_000_000 + i).to_le_bytes());
        data.extend(536_870_912i32.to_le_bytes()); // 45 degrees
        data.extend((-268_435_456i32).to_le_bytes()); // -22.5 degrees
        data.extend(3000u16.to_le_bytes()); // altitude 3000/5 - 500 = 100 m
        data.extend([120 + i as u8, 80]);
        data.extend(5000u16.to_le_bytes()); // 5 m/s, overridden by enhanced speed
        data.extend(250u16.to_le_bytes());
        data.push((-5i8) as u8);
        data.extend(6000u32.to_le_bytes()); // 6 m/s
    }
    let mut out = vec![12, 0x20, 0, 0];
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(b".FIT");
    out.extend(data);
    out.extend(crc(&out).to_le_bytes());
    out
}
#[test]
fn fit_scaling_enhanced_fields_temperature_and_crc_validation() {
    let mut bytes = fit();
    let a = Activity::from_fit(Cursor::new(&bytes)).unwrap();
    let tel = a.standalone();
    let s = tel.sample(0.0);
    for (m, v) in [
        (Metric::Lat, 45.0),
        (Metric::Lon, -22.5),
        (Metric::Alt, 100.0),
        (Metric::Speed, 6.0),
        (Metric::Hr, 120.0),
        (Metric::Cadence, 80.0),
        (Metric::Power, 250.0),
        (Metric::Temp, -5.0),
    ] {
        assert_eq!(s.get(m), Value::Present(v), "{m:?}");
    }
    bytes[20] ^= 1;
    assert!(Activity::from_fit(Cursor::new(&bytes)).is_err());
}

#[test]
fn public_garmin_activity_when_downloaded() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/external/Activity.fit");
    if !p.exists() {
        eprintln!("public Garmin sample absent; run scripts/fetch-external-samples.sh");
        return;
    }
    let a = Activity::read(&p).unwrap();
    assert!(a.points.len() > 10);
    let tel = a.standalone();
    assert!(tel.availability().is_available(Metric::Speed));
    assert!(tel.availability().is_available(Metric::Hr));
}

#[test]
fn public_garmin_gears_when_downloaded() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/external/WithGearChangeData.fit");
    if !p.exists() {
        return;
    }
    let a = Activity::read(&p).unwrap();
    let t = a.standalone();
    assert!(t.availability().is_available(Metric::GearRear));
    assert_eq!(t.sample(0.0).get(Metric::GearRear), Value::Present(24.0));
    assert_eq!(t.sample(50.0).get(Metric::GearRear), Value::Present(21.0));
}
