//! Comparison with gopro-dashboard-overlay 0.134.0 on public GoPro samples.
//! How the reference files were made: tests/reference/README.md.
mod common;

use std::collections::HashMap;

use actionlay_telemetry::{Derived, GpsPoint, Metric, Telemetry};
use chrono::DateTime;
use common::{key, num, reference, stats};

const SAMPLES: [&str; 3] = ["hero5", "hero6", "max-heromode"];

fn by_key(tel: &Telemetry) -> HashMap<(usize, usize), &GpsPoint> {
    tel.gps_points()
        .iter()
        .map(|p| ((p.packet, p.index), p))
        .collect()
}

fn assert_close(what: &str, ours: f64, theirs: f64, tol: f64) {
    assert!(
        (ours - theirs).abs() <= tol,
        "{what}: ours {ours}, original {theirs}, tolerance {tol}"
    );
}

/// Raw values parsed from GPMF must equal gopro-to-csv's.
#[test]
fn raw_gps_matches_gopro_to_csv() {
    for s in SAMPLES {
        let Some(tel) = common::load(&format!("{s}.mp4")) else {
            continue;
        };
        let ours = by_key(&tel);
        let rows = reference(&format!("{s}.gopro-to-csv.csv"));
        let last_packet = rows.iter().map(|r| key(r).0).max().unwrap();
        for r in &rows {
            let at = format!("{s} {:?}", key(r));
            // every reference row has a match; we may have more (the
            // original drops samples whose timestamps collide)
            let p = ours.get(&key(r)).unwrap_or_else(|| panic!("{at}: missing"));
            assert_eq!(p.lock.original_name(), r["gps_fix"], "{at}");
            assert_close(&format!("{at} lat"), p.lat, num(r, "lat").unwrap(), 1e-7);
            assert_close(&format!("{at} lon"), p.lon, num(r, "lon").unwrap(), 1e-7);
            assert_close(&format!("{at} dop"), p.dop, num(r, "dop").unwrap(), 1e-6);
            if p.lock.is_locked() {
                assert_close(&format!("{at} alt"), p.alt, num(r, "alt").unwrap(), 1e-3);
                assert_close(
                    &format!("{at} speed"),
                    p.speed2d,
                    num(r, "speed").unwrap(),
                    1e-3,
                );
            } else {
                assert_eq!(num(r, "alt"), None, "{at}");
                assert_eq!(num(r, "speed"), None, "{at}");
            }
            // UTC: equal to GPSU at the first sample of a packet; within
            // 0.1 s elsewhere (the original spaces samples by its own clock
            // model), except in the final short packet, which the original
            // stretches past the end of the file.
            let theirs = DateTime::parse_from_rfc3339(&r["date"].replacen(' ', "T", 1)).unwrap();
            let diff = (p.utc.unwrap() - theirs.to_utc()).as_seconds_f64().abs();
            if p.index == 0 {
                assert!(diff < 1e-3, "{at} date off by {diff}");
            } else if p.packet != last_packet {
                assert!(diff < 0.1, "{at} date off by {diff}");
            }
        }
        assert!(tel.gps_points().len() >= rows.len());
    }
}

type Getter = fn(&Derived) -> Option<f64>;

/// (column, tolerance, value). Tolerances, measured on these samples plus a
/// margin:
/// - lat/lon 2e-6°: identical on HERO5/6; on MAX the original sorts samples
///   by its STMP clock, which moves one sample per packet boundary across
///   the next packet's first sample and shifts its smoothing slightly.
/// - speed 0.02 m/s, cspeed 0.15 m/s, accel 0.05 m/s²: our UTC spacing of
///   samples (packet duration / n) differs from the original's by up to
///   ~2 % over a 3 s window; the Kalman filters carry that along.
/// - dist 1 mm, codo 0.2 m, azi/cog 1°, cgrad 1 %: same causes.
const DERIVED: [(&str, f64, Getter); 11] = [
    ("lat", 2e-6, |d| d.lat),
    ("lon", 2e-6, |d| d.lon),
    ("alt", 1e-3, |d| d.alt),
    ("speed", 0.02, |d| d.speed),
    ("cspeed", 0.15, |d| d.cspeed),
    ("dist", 1e-3, |d| d.dist),
    ("codo", 0.2, |d| d.codo),
    ("azi", 1.0, |d| d.azi),
    ("cog", 1.0, |d| d.cog),
    ("cgrad", 1.0, |d| d.cgrad),
    ("accel", 0.05, |d| d.accel),
];

/// Derived values must match what the original's dashboard pipeline shows.
#[test]
fn derived_metrics_match_the_dashboard() {
    for s in SAMPLES {
        let Some(tel) = common::load(&format!("{s}.mp4")) else {
            continue;
        };
        let ours = by_key(&tel);
        for r in &reference(&format!("{s}.dashboard.csv")) {
            let at = format!("{s} {:?}", key(r));
            let p = ours.get(&key(r)).unwrap_or_else(|| panic!("{at}: missing"));
            if !p.lock.is_locked() {
                assert_eq!(p.derived, Derived::default(), "{at}");
                continue;
            }
            for (col, tol, get) in DERIVED {
                match (get(&p.derived), num(r, col)) {
                    (None, None) => {}
                    (Some(a), Some(b)) => {
                        let mut d = (a - b).abs();
                        if col == "azi" || col == "cog" {
                            d = d.min(360.0 - d);
                        }
                        assert!(d <= tol, "{at} {col}: ours {a}, original {b}, tol {tol}");
                    }
                    (a, b) => panic!("{at} {col}: ours {a:?}, original {b:?}"),
                }
            }
        }
    }
}

/// The accelerometer as displayed: same decimation and Kalman filter as the
/// original, but sampled on our clock, so compared statistically.
#[test]
fn accelerometer_is_close_to_the_original() {
    for s in SAMPLES {
        let Some(tel) = common::load(&format!("{s}.mp4")) else {
            continue;
        };
        let ours = by_key(&tel);
        let mut errors = Vec::new();
        for r in &reference(&format!("{s}.gopro-to-csv.csv")) {
            let p = ours[&key(r)];
            let snap = tel.sample(p.t);
            for (m, col) in [
                (Metric::AcclX, "accl_x"),
                (Metric::AcclY, "accl_y"),
                (Metric::AcclZ, "accl_z"),
            ] {
                let a = snap.get(m).present().unwrap();
                errors.push((a - num(r, col).unwrap()).abs());
            }
        }
        let (mean, p95) = stats(errors);
        assert!(mean < 0.25 && p95 < 0.8, "{s}: mean {mean}, p95 {p95}");
    }
}

#[test]
fn gravity_and_orientation_are_close_to_the_original() {
    let Some(tel) = common::load("max-heromode.mp4") else {
        return;
    };
    let ours = by_key(&tel);
    let (mut grav, mut ori) = (Vec::new(), Vec::new());
    for r in &reference("max-heromode.dashboard.csv") {
        let snap = tel.sample(ours[&key(r)].t);
        for (m, col) in [
            (Metric::GravX, "grav_x"),
            (Metric::GravY, "grav_y"),
            (Metric::GravZ, "grav_z"),
        ] {
            grav.push((snap.get(m).present().unwrap() - num(r, col).unwrap()).abs());
        }
        for (m, col) in [
            (Metric::OriPitch, "ori_pitch"),
            (Metric::OriRoll, "ori_roll"),
            (Metric::OriYaw, "ori_yaw"),
        ] {
            let d = (snap.get(m).present().unwrap() - num(r, col).unwrap().to_degrees()).abs();
            ori.push(d.min(360.0 - d));
        }
    }
    let (gm, gp) = stats(grav);
    assert!(gm < 0.01 && gp < 0.03, "grav mean {gm}, p95 {gp}");
    let (om, op) = stats(ori);
    assert!(om < 0.5 && op < 2.0, "ori mean {om}°, p95 {op}°");
}
