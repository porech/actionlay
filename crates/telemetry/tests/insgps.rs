use actionlay_telemetry::{
    Metric, Value,
    external::{Activity, ExternalError},
};

fn record(seconds: u64, millis: u16, fix: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(seconds.to_le_bytes());
    bytes.extend(millis.to_le_bytes());
    bytes.push(fix);
    bytes.extend(45.0f64.to_le_bytes());
    bytes.push(b'S');
    bytes.extend(7.0f64.to_le_bytes());
    bytes.push(b'W');
    bytes.extend(0.0f64.to_le_bytes());
    bytes.extend(125.0f64.to_le_bytes());
    bytes.extend(1200.0f64.to_le_bytes());
    bytes
}

#[test]
fn insgps_preserves_milliseconds_coordinates_zero_speed_and_void_fixes() {
    let mut bytes = record(1_788_099_905, 823, b'A');
    bytes.extend(record(1_788_099_906, 328, b'V'));
    let activity = Activity::from_bytes("PHONE.INSGPS", &bytes).unwrap();
    assert_eq!(activity.points.len(), 2);
    assert_eq!(activity.points[0].utc.timestamp_subsec_millis(), 823);
    assert_eq!(activity.points[0].values[&Metric::Lat], -45.0);
    assert_eq!(activity.points[0].values[&Metric::Lon], -7.0);
    assert_eq!(activity.points[0].values[&Metric::Speed], 0.0);
    assert_eq!(activity.points[0].values[&Metric::Cog], 125.0);
    assert!(!activity.points[1].values.contains_key(&Metric::Lat));
    assert!(!activity.points[1].values.contains_key(&Metric::Speed));
    assert_eq!(
        activity.standalone().sample(0.0).get(Metric::Speed),
        Value::Present(0.0)
    );
}

#[test]
fn truncated_and_unknown_insgps_records_are_rejected() {
    let mut bytes = record(1_788_099_905, 823, b'A');
    assert!(matches!(
        Activity::from_insgps(&bytes[..52]),
        Err(ExternalError::InvalidInsgps)
    ));
    bytes[10] = b'X';
    assert!(matches!(
        Activity::from_insgps(&bytes),
        Err(ExternalError::InvalidInsgps)
    ));
    let bytes = record(u64::MAX, 823, b'A');
    assert!(matches!(
        Activity::from_insgps(&bytes),
        Err(ExternalError::InvalidInsgps)
    ));
    assert!(matches!(
        Activity::from_insgps(&record(1_788_099_905, 1000, b'A')),
        Err(ExternalError::InvalidInsgps)
    ));
}

#[test]
fn timestamp_matching_and_explicit_start_fallback_do_not_depend_on_offset() {
    let mut bytes = record(1_788_099_905, 823, b'A');
    bytes.extend(record(1_788_099_909, 0, b'A'));
    let activity = Activity::from_insgps(&bytes).unwrap();
    let start = activity.points[0].utc;
    assert!(activity.overlaps_video(Some(start), 2.0));
    assert!(!activity.overlaps_video(Some(start + chrono::Duration::seconds(100)), 2.0));
    assert!(!activity.overlaps_video(None, 2.0));
    assert_eq!(activity.sync_origin(Some(start), 2.0), (start, false));
    assert_eq!(activity.sync_origin(None, 2.0), (start, true));
    assert_eq!(
        activity.sync_origin(Some(start + chrono::Duration::days(32)), 2.0),
        (start, true)
    );
    let tel = activity
        .align(Some(activity.sync_origin(None, 2.0).0), 2.0, 1.0)
        .unwrap();
    assert_eq!(tel.sample(0.0).get(Metric::Speed), Value::Absent);
    assert_eq!(tel.sample(1.0).get(Metric::Speed), Value::Present(0.0));
}
