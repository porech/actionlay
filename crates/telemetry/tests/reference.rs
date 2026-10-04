//! Comparison with gopro-dashboard-overlay 0.134.0 on public GoPro samples.
//! How the reference files were made: tests/reference/README.md.
mod common;

use std::collections::HashMap;

use actionlay_telemetry::{Derived, GpsPoint, Metric, Telemetry};
use chrono::DateTime;
use common::{key, num, reference, stats};

const SAMPLES: [&str; 3] = ["hero5", "hero6", "max-heromode"];

fn by_key(tel: &Telemetry) -> HashMap<(usize, usize), &GpsPoint> {
    let map: HashMap<_, _> = tel
        .gps_points()
        .iter()
        .map(|p| ((p.packet, p.index), p))
        .collect();
    assert_eq!(
        map.len(),
        tel.gps_points().len(),
        "duplicate (packet, index)"
    );
    map
}

/// (sample, reference rows, our points, keys only we have). The original's
/// `items()` skips two MAX samples whose timestamps fall out of order; we keep
/// them (see tests/reference/README.md).
type Expected = (&'static str, usize, usize, &'static [(usize, usize)]);
const EXPECTED: [Expected; 3] = [
    ("hero5", 618, 618, &[]),
    ("hero6", 417, 417, &[]),
    ("max-heromode", 189, 191, &[(0, 16), (4, 0)]),
];

/// Largest absolute difference per column, and how many cells were compared.
#[derive(Default)]
struct Maxima(std::collections::BTreeMap<&'static str, (f64, usize)>);

impl Maxima {
    fn check(&mut self, col: &'static str, at: &str, ours: f64, theirs: f64, tol: f64) {
        let d = (ours - theirs).abs();
        assert!(
            d <= tol,
            "{at} {col}: ours {ours}, original {theirs}, tolerance {tol}"
        );
        let e = self.0.entry(col).or_insert((0.0, 0));
        e.0 = e.0.max(d);
        e.1 += 1;
    }

    fn report(&self, what: &str) -> usize {
        let total = self.0.values().map(|v| v.1).sum();
        for (col, (max, n)) in &self.0 {
            eprintln!("{what} {col}: max |diff| {max:e} over {n} cells");
        }
        assert!(total > 0, "{what}: nothing was compared");
        total
    }
}

/// Raw values parsed from GPMF must equal gopro-to-csv's.
#[test]
fn raw_gps_matches_gopro_to_csv() {
    for (s, expected_rows, expected_points, extra) in EXPECTED {
        let Some(tel) = common::load(&format!("{s}.mp4")) else {
            continue;
        };
        let ours = by_key(&tel);
        let mut m = Maxima::default();
        let rows = reference(&format!("{s}.gopro-to-csv.csv"));
        let last_packet = rows.iter().map(|r| key(r).0).max().unwrap();
        for r in &rows {
            let at = format!("{s} {:?}", key(r));
            // every reference row has a match; we may have more (the
            // original drops samples whose timestamps collide)
            let p = ours.get(&key(r)).unwrap_or_else(|| panic!("{at}: missing"));
            assert_eq!(p.lock.original_name(), r["gps_fix"], "{at}");
            m.check("lat", &at, p.lat, num(r, "lat").unwrap(), 1e-7);
            m.check("lon", &at, p.lon, num(r, "lon").unwrap(), 1e-7);
            m.check("dop", &at, p.dop, num(r, "dop").unwrap(), 1e-6);
            if p.lock.is_locked() {
                // measured max: 0 on all samples for lat/lon/dop/alt/speed (1e-7, 1e-6, 1e-3
                // are the CSV print-precision floors)
                m.check("alt", &at, p.alt, num(r, "alt").unwrap(), 1e-3);
                m.check("speed", &at, p.speed2d, num(r, "speed").unwrap(), 1e-3);
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
                // measured max: 0 at index 0; elsewhere 0.062 (hero5), 0.059 (hero6), 0.060 (max)
                m.check("date(index 0)", &at, diff, 0.0, 1e-3);
            } else if p.packet != last_packet {
                m.check("date", &at, diff, 0.0, 0.1);
            }
        }
        assert_eq!(rows.len(), expected_rows, "{s}: reference rows");
        assert_eq!(tel.gps_points().len(), expected_points, "{s}: our points");
        let theirs: std::collections::HashSet<_> = rows.iter().map(key).collect();
        let mut only_ours: Vec<_> = ours.keys().filter(|k| !theirs.contains(k)).collect();
        only_ours.sort();
        assert_eq!(
            only_ours,
            extra.iter().collect::<Vec<_>>(),
            "{s}: extra keys"
        );
        m.report(s);
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
//
// Measured maxima (fix round 1) are in the trailing comments.
const DERIVED: [(&str, f64, Getter); 11] = [
    ("lat", 2e-6, |d| d.lat), // measured max: 5.3e-7 (max-heromode); 0 on hero5/6
    ("lon", 2e-6, |d| d.lon), // measured max: 4.7e-7 (max-heromode); 0 on hero5/6
    ("alt", 1e-3, |d| d.alt), // measured max: 0 (all)
    ("speed", 0.02, |d| d.speed), // measured max: 7.3e-3 (max-heromode); 0 on hero5/6
    ("cspeed", 0.15, |d| d.cspeed), // measured max: 0.102 (max-heromode); 0.046 hero5, 0.031 hero6
    ("dist", 1e-3, |d| d.dist), // measured max: 2.5e-4 (max-heromode); <6e-11 hero5/6
    ("codo", 0.2, |d| d.codo), // measured max: 0.074 (max-heromode); <1e-9 hero5/6
    ("azi", 1.0, |d| d.azi),  // measured max: 0.306 (max-heromode); <7e-8 hero5/6
    ("cog", 1.0, |d| d.cog),  // measured max: 0.306 (max-heromode); <7e-8 hero5/6
    ("cgrad", 1.0, |d| d.cgrad), // measured max: 0.453 (max-heromode); <2e-8 hero5/6
    ("accel", 0.05, |d| d.accel), // measured max: 0.0194 (max-heromode); 0.0118 hero5, 0.0177 hero6
];

/// Derived values must match what the original's dashboard pipeline shows.
#[test]
fn derived_metrics_match_the_dashboard() {
    for s in SAMPLES {
        let Some(tel) = common::load(&format!("{s}.mp4")) else {
            continue;
        };
        let ours = by_key(&tel);
        let mut m = Maxima::default();
        let mut locked_rows = 0;
        let rows = reference(&format!("{s}.dashboard.csv"));
        for r in &rows {
            let at = format!("{s} {:?}", key(r));
            let p = ours.get(&key(r)).unwrap_or_else(|| panic!("{at}: missing"));
            if !p.lock.is_locked() {
                assert_eq!(p.derived, Derived::default(), "{at}");
                continue;
            }
            locked_rows += 1;
            if let Some(a) = p.derived.azi {
                assert!(a > -180.0 && a <= 180.0, "{at} azi {a} out of (-180, 180]");
            }
            if let Some(c) = p.derived.cog {
                assert!((0.0..360.0).contains(&c), "{at} cog {c} out of [0, 360)");
            }
            for (col, tol, get) in DERIVED {
                match (get(&p.derived), num(r, col)) {
                    (None, None) => {}
                    (Some(a), Some(b)) => {
                        if col == "azi" || col == "cog" {
                            m.check(col, &at, common::angle_diff(a, b), 0.0, tol);
                        } else {
                            m.check(col, &at, a, b, tol);
                        }
                    }
                    (a, b) => panic!("{at} {col}: ours {a:?}, original {b:?}"),
                }
            }
        }
        assert!(locked_rows > 0, "{s}: no locked rows compared");
        m.report(s);
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
        assert!(errors.len() >= 3 * 189, "{s}: only {} cells", errors.len());
        let (mean, p95) = stats(errors);
        eprintln!("{s}: accel mean {mean}, p95 {p95}");
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
            let ours = snap.get(m).present().unwrap();
            if m == Metric::OriYaw {
                assert!(
                    ours > -180.0 && ours <= 180.0,
                    "ori.yaw {ours} out of (-180, 180]"
                );
            }
            ori.push(common::angle_diff(ours, num(r, col).unwrap().to_degrees()));
        }
    }
    assert!(grav.len() == 3 * 189 && ori.len() == 3 * 189, "row count");
    let (gm, gp) = stats(grav);
    eprintln!(
        "grav mean {gm}, p95 {gp}; ori mean {}, p95 {}",
        stats(ori.clone()).0,
        stats(ori.clone()).1
    );
    assert!(gm < 0.01 && gp < 0.03, "grav mean {gm}, p95 {gp}");
    let (om, op) = stats(ori);
    assert!(om < 0.5 && op < 2.0, "ori mean {om}°, p95 {op}°");
}

/// Metric ids accepted by `metric_accessor_from` in gopro_overlay/layout_xml.py
/// of gopro-dashboard-overlay 0.134.0, except the three listed in
/// `LEFT_OUT`. Every one must be a metric of ours.
const ORIGINAL_METRIC_IDS: [&str; 28] = [
    "hr",
    "cadence",
    "power",
    "speed",
    "cspeed",
    "accel",
    "temp",
    "gradient",
    "cgrad",
    "alt",
    "odo",
    "codo",
    "dist",
    "azi",
    "cog",
    "gps-dop",
    "gps-lock",
    "respiration",
    "gear.front",
    "gear.rear",
    "accl.x",
    "accl.y",
    "accl.z",
    "grav.x",
    "grav.y",
    "grav.z",
    "ori.pitch",
    "ori.roll",
];

/// Deliberately not metrics (plan, "deliberate differences"): the time of day
/// is `Snapshot::utc`, packet and index are debugging data in `gps_points()`.
const LEFT_OUT: [&str; 3] = ["timestamp", "gps-packet", "gps-packet-index"];

#[test]
fn every_metric_of_the_original_exists() {
    let all = ORIGINAL_METRIC_IDS
        .iter()
        .chain(&["ori.yaw", "lat", "lon", "sdps"]);
    for id in all {
        let m = Metric::from_id(id).unwrap_or_else(|| panic!("no metric {id}"));
        assert_eq!(m.id(), *id);
    }
    for id in LEFT_OUT {
        assert!(
            Metric::from_id(id).is_none(),
            "{id} is now a metric: update the list"
        );
    }
}
