use actionlay_telemetry::{Metric, Value, camera};
use std::sync::{Arc, atomic::AtomicBool};
#[test]
fn camera_adapter_normalizes_accelerometer_units_and_axes() {
    let path = std::env::temp_dir().join(format!("actionlay-imu-{}.gcsv", std::process::id()));
    std::fs::write(&path,"GYROFLOW IMU LOG\nversion,1.3\norientation,YxZ\ntscale,0.001\nascale,0.5\nt,gx,gy,gz,ax,ay,az\n0,0,0,0,2,4,6\n10,0,0,0,2,4,6\n20,0,0,0,2,4,6\n").unwrap();
    let result = camera::read(&path, 1.0, Arc::new(AtomicBool::new(false)));
    std::fs::remove_file(path).unwrap();
    let tel = result.unwrap();
    // gcsv stores acceleration in g; ascale and orientation are applied upstream.
    let snapshot = tel.sample(0.01);
    // Orientation YxZ maps [2,4,6] to [4,-2,6]; scale 0.5 g becomes m/s².
    for (metric, expected) in [
        (Metric::AcclX, 2.0 * 9.80665),
        (Metric::AcclY, -9.80665),
        (Metric::AcclZ, 3.0 * 9.80665),
    ] {
        assert!(
            (snapshot.get(metric).present().unwrap() - expected).abs() < 1e-8,
            "{metric:?}: {:?}",
            snapshot.get(metric)
        );
    }
    assert_eq!(tel.sample(0.01).get(Metric::Lat), Value::Absent);
    assert!(!tel.has_imu_acceleration());
    assert!(tel.sample(0.5).get(Metric::AcclX).present().is_none());
}

#[test]
fn public_dji_avata_orientation_when_downloaded() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/cameras/dji-avata.mp4");
    if !path.exists() {
        return;
    }
    let duration = actionlay_media::probe::probe(&path).unwrap().duration;
    let tel = camera::read(&path, duration, Arc::new(AtomicBool::new(false))).unwrap();
    for metric in [Metric::OriPitch, Metric::OriRoll, Metric::OriYaw] {
        assert!(tel.availability().coverage(metric) > 0.99);
        assert!(
            tel.sample(duration * 0.5)
                .get(metric)
                .present()
                .unwrap()
                .is_finite()
        );
        assert!(tel.sample(duration + 1.0).get(metric).present().is_none());
    }
    assert_eq!(tel.sample(1.0).get(Metric::Lat), Value::Absent);
    assert_eq!(tel.sample(1.0).get(Metric::AcclX), Value::Absent);
}

#[test]
fn public_insta360_onex2_accelerometer_when_downloaded() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/cameras/insta360-onex2.insv");
    if !path.exists() {
        return;
    }
    let duration = actionlay_media::probe::probe(&path).unwrap().duration;
    let tel = camera::read(&path, duration, Arc::new(AtomicBool::new(false))).unwrap();
    for metric in [Metric::AcclX, Metric::AcclY, Metric::AcclZ] {
        assert!(tel.availability().coverage(metric) > 0.99);
        assert!(
            tel.sample(duration * 0.5)
                .get(metric)
                .present()
                .unwrap()
                .is_finite()
        );
        assert!(tel.sample(duration + 1.0).get(metric).present().is_none());
    }
    let at = tel.sample(0.25);
    let norm = [Metric::AcclX, Metric::AcclY, Metric::AcclZ]
        .iter()
        .map(|m| at.get(*m).present().unwrap().powi(2))
        .sum::<f64>()
        .sqrt();
    assert!(
        (6.0..16.0).contains(&norm),
        "accelerometer must be in m/s²: {norm}"
    );
    assert_eq!(tel.sample(0.25).get(Metric::Lat), Value::Absent);
}
