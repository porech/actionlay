//! Additions to the M1/M2 contract that M2 relies on.
use actionlay_telemetry::metric::Metric;
use actionlay_telemetry::{GpsLock, Snapshot, Value};
use chrono::{TimeZone, Utc};

#[test]
fn for_test_snapshot_returns_given_values_and_absent_otherwise() {
    let speed = Metric::from_id("speed").unwrap();
    let alt = Metric::from_id("alt").unwrap();
    let utc = Utc.with_ymd_and_hms(2026, 9, 27, 12, 15, 30).unwrap();
    let snap = Snapshot::for_test(
        1.5,
        Some(utc),
        GpsLock::Lock3d,
        &[
            (speed, Value::Present(10.0)),
            (
                alt,
                Value::Stale {
                    value: 300.0,
                    age: 2.0,
                },
            ),
        ],
    );
    assert_eq!(snap.t, 1.5);
    assert_eq!(snap.utc, Some(utc));
    assert_eq!(snap.gps_lock, GpsLock::Lock3d);
    assert_eq!(snap.get(speed), Value::Present(10.0));
    assert_eq!(
        snap.get(alt),
        Value::Stale {
            value: 300.0,
            age: 2.0
        }
    );
    assert_eq!(snap.get(Metric::from_id("temp").unwrap()), Value::Absent);
}

#[test]
fn for_test_marks_listed_metrics_available_only() {
    let speed = Metric::Speed;
    let snap = Snapshot::for_test(0.0, None, GpsLock::NoLock, &[(speed, Value::Present(1.0))]);
    for m in Metric::ALL {
        assert_eq!(snap.is_available(m), m == speed, "metric {}", m.id());
    }
    assert_eq!(snap.utc, None);
}
