//! Causal course and planar vehicle acceleration, independent of progressive read tails.
use crate::{
    GpsPoint, Metric, Value,
    series::{Interp, Series},
};
use geographiclib_rs::{Geodesic, InverseGeodesic};

fn present(series: &[Option<Series>], m: Metric, t: f64) -> Option<f64> {
    match series[m.index()].as_ref()?.sample(t) {
        Value::Present(v) => Some(v),
        _ => None,
    }
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn normal(v: [f64; 3]) -> Option<[f64; 3]> {
    let n = dot(v, v).sqrt();
    (n > 1e-6 && n.is_finite()).then(|| v.map(|x| x / n))
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn add(g: &[GpsPoint], series: &mut [Option<Series>]) -> Option<f64> {
    add_with_breaks(g, series, &[])
}
pub(crate) fn add_with_breaks(
    g: &[GpsPoint],
    series: &mut [Option<Series>],
    breaks: &[f64],
) -> Option<f64> {
    if g.is_empty() {
        return None;
    }
    let mut heading = Vec::new();
    let mut segment = 0;
    let mut previous = None;
    // A forward-looking pair changes meaning at the end of a progressively
    // loaded prefix. Use only positions already observed at this timestamp,
    // so receiving another packet never revises an earlier compass heading.
    for (i, p) in g.iter().enumerate() {
        let valid = p.derived.lat.zip(p.derived.lon);
        if valid.is_none() {
            segment = i + 1;
            previous = None;
        } else if i > 0
            && (p.t - g[i - 1].t > 2.0 || breaks.iter().any(|&b| b > g[i - 1].t && b <= p.t))
        {
            segment = i;
            previous = None;
        }
        let j = g[..i].partition_point(|q| q.t < p.t - 1.0).max(segment);
        let value = if j < i
            && let (Some((lat, lon)), Some(a), Some(b)) =
                (valid, g[j].derived.lat, g[j].derived.lon)
        {
            let (distance, azimuth, _, _): (f64, f64, f64, f64) =
                Geodesic::wgs84().inverse(a, b, lat, lon);
            if distance > 0.3 {
                previous = Some(azimuth.rem_euclid(360.0));
            }
            previous
        } else {
            None
        };
        heading.push((p.t, p.end, value));
    }
    series[Metric::Heading.index()] =
        Some(Series::new(Interp::Angle360, heading).with_breaks(breaks.to_vec()));
    let mut long = Vec::new();
    let mut lat = Vec::new();
    let mut calibrated: Option<([f64; 3], [f64; 3])> = None;
    let mut calibrated_at = None;
    let mut evidence = 0;
    let mut first_evidence = None;
    let mut accel_energy = 0.0;
    let mut gps_energy = 0.0;
    let mut gravity = [0.0; 3];
    let mut filtered = [0.0; 3];
    let mut last_t = None;
    let mut forward = [0.0; 3];
    let mut mean_gravity = [0.0; 3];
    for p in g {
        let t = p.t;
        let before = t - 0.25;
        let connected = !breaks.iter().any(|&b| b > before && b <= t);
        let pair = connected
            .then(|| present(series, Metric::Speed, t).zip(present(series, Metric::Speed, before)))
            .flatten();
        let lon = pair.map(|(v, a)| (v - a) / 0.25);
        let sideways = connected
            .then(|| present(series, Metric::Heading, t))
            .flatten()
            .zip(present(series, Metric::Heading, before))
            .zip(present(series, Metric::Speed, t))
            .map(|((a, b), v)| v * ((a - b + 180.0).rem_euclid(360.0) - 180.0).to_radians() / 0.25);
        long.push((t, p.end, lon));
        lat.push((t, p.end, sideways));
        let accel = [Metric::AcclX, Metric::AcclY, Metric::AcclZ].map(|m| present(series, m, t));
        if let [Some(x), Some(y), Some(z)] = accel {
            let v = [x, y, z];
            let dt = last_t.map_or(1.0, |last: f64| (t - last).max(0.0));
            if last_t.is_none() || dt > 2.0 {
                gravity = v;
                filtered = v;
            }
            let slow = 1.0 - (-dt / 2.0).exp();
            let fast = 1.0 - (-dt / 0.08).exp();
            for k in 0..3 {
                gravity[k] += slow * (v[k] - gravity[k]);
                filtered[k] += fast * (v[k] - filtered[k]);
            }
            let grav = [Metric::GravX, Metric::GravY, Metric::GravZ].map(|m| present(series, m, t));
            let up = if let [Some(x), Some(y), Some(z)] = grav {
                normal([x, y, z])
            } else {
                normal(gravity)
            };
            if let Some(up) = up {
                let vertical = dot(filtered, up);
                let h = std::array::from_fn(|k| filtered[k] - vertical * up[k]);
                if let Some(lon) = lon.filter(|v| v.abs() > 0.2) {
                    for k in 0..3 {
                        forward[k] += h[k] * lon;
                    }
                }
                for k in 0..3 {
                    mean_gravity[k] += up[k];
                }
                if calibrated.is_none() {
                    if let Some(lon) = lon.filter(|v| v.abs() > 0.2) {
                        evidence += 1;
                        first_evidence.get_or_insert(t);
                        accel_energy += dot(h, h);
                        gps_energy += lon * lon;
                    }
                    // Freeze the mounting basis at the first confident estimate.
                    // Never use later packets to retroactively rotate past values.
                    let coherence =
                        dot(forward, forward).sqrt() / (accel_energy * gps_energy).sqrt().max(1e-9);
                    if evidence >= 36 && t - first_evidence.unwrap_or(t) >= 2.0 && coherence > 0.55
                    {
                        calibrated =
                            normal(forward)
                                .zip(normal(mean_gravity))
                                .and_then(|(f, up)| {
                                    normal(std::array::from_fn(|k| f[k] - dot(f, up) * up[k]))
                                        .map(|f| (f, cross(f, up)))
                                });
                        if calibrated.is_some() {
                            calibrated_at = Some(t);
                        }
                    }
                }
                if let Some((f, side)) = calibrated
                    && lon.is_some()
                    && sideways.is_some()
                {
                    long.last_mut().unwrap().2 = Some(dot(h, f));
                    lat.last_mut().unwrap().2 = Some(dot(h, side));
                }
            }
            last_t = Some(t);
        }
    }
    series[Metric::AccelLon.index()] =
        Some(Series::new(Interp::Linear, long).with_breaks(breaks.to_vec()));
    series[Metric::AccelLat.index()] =
        Some(Series::new(Interp::Linear, lat).with_breaks(breaks.to_vec()));
    calibrated_at
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GpsLock, derive, extract::Derived};

    fn turning_track(count: usize) -> Vec<GpsPoint> {
        (0..count)
            .map(|i| {
                let t = i as f64 / 18.0;
                // A smooth turn, with no GPS noise or discontinuities.
                let angle = t * 0.25;
                GpsPoint {
                    packet: i / 18,
                    index: i % 18,
                    t,
                    end: t + 1.0 / 18.0,
                    utc: None,
                    lat: 45.0 + angle.sin() * 0.001,
                    lon: 7.0 + (1.0 - angle.cos()) * 0.0014,
                    alt: 100.0,
                    speed2d: 20.0,
                    speed3d: 20.0,
                    fix: 3,
                    dop: 1.0,
                    lock: GpsLock::Lock3d,
                    derived: Derived::default(),
                }
            })
            .collect()
    }

    fn processed(mut points: Vec<GpsPoint>) -> (Vec<GpsPoint>, Vec<Option<Series>>) {
        derive::derive(&mut points);
        let mut series = vec![None; Metric::COUNT];
        add(&points, &mut series);
        (points, series)
    }

    fn delta(a: f64, b: f64) -> f64 {
        (a - b + 180.0).rem_euclid(360.0) - 180.0
    }

    fn imu_track(count: usize, stationary: bool) -> (Vec<GpsPoint>, Vec<Option<Series>>) {
        let mut points = turning_track(count);
        for p in &mut points {
            p.derived.lat = Some(p.lat);
            p.derived.lon = Some(p.lon);
        }
        let mut series = vec![None; Metric::COUNT];
        for (metric, axis) in [
            (Metric::Speed, 0),
            (Metric::AcclX, 1),
            (Metric::AcclY, 2),
            (Metric::AcclZ, 3),
            (Metric::GravX, 4),
            (Metric::GravY, 4),
            (Metric::GravZ, 5),
        ] {
            series[metric.index()] = Some(Series::new(
                Interp::Linear,
                points.iter().map(|p| {
                    // Camera mounted sideways: forward acceleration is camera Y.
                    let a = if stationary {
                        0.0
                    } else {
                        2.0 * (p.t * 0.7).cos()
                    };
                    let v = match axis {
                        0 => {
                            if stationary {
                                0.0
                            } else {
                                10.0 + 2.0 / 0.7 * (p.t * 0.7).sin()
                            }
                        }
                        2 => a,
                        3 => 9.80665,
                        5 => 1.0,
                        _ => 0.0,
                    };
                    (p.t, p.end, Some(v))
                }),
            ));
        }
        (points, series)
    }

    #[test]
    fn rotated_imu_calibrates_causally_and_removes_gravity() {
        let (points, mut full) = imu_track(360, false);
        let calibration = add(&points, &mut full).expect("moving camera should calibrate");
        assert!(calibration >= 2.0);
        for count in (72..360).step_by(18) {
            let (points, mut prefix) = imu_track(count, false);
            add(&points, &mut prefix);
            for p in &points {
                for m in [Metric::AccelLon, Metric::AccelLat] {
                    assert_eq!(
                        present(&prefix, m, p.t),
                        present(&full, m, p.t),
                        "{m:?} at {} in prefix {count}",
                        p.t
                    );
                }
            }
        }
        let t = 10.0;
        let long = present(&full, Metric::AccelLon, t).unwrap();
        assert!((long - 2.0 * (t * 0.7).cos()).abs() < 0.2, "{long}");
        assert!(present(&full, Metric::AccelLat, t).unwrap().abs() < 1e-6);
    }

    #[test]
    fn stationary_imu_cannot_invent_a_mounting_orientation() {
        let (points, mut series) = imu_track(180, true);
        assert_eq!(add(&points, &mut series), None);
        assert_eq!(present(&series, Metric::AccelLon, 5.0), Some(0.0));
    }

    #[test]
    fn progressive_tail_revises_legacy_course_even_without_gps_noise() {
        let (short, _) = processed(turning_track(180));
        let (long, _) = processed(turning_track(270));
        // At the same instant the old calculation switches from a backward
        // pair to a forward pair when additional packets become available.
        let i = 150;
        let change = delta(short[i].derived.cog.unwrap(), long[i].derived.cog.unwrap());
        assert!(
            change.abs() > 30.0,
            "expected to reproduce the spike: {change}"
        );
    }

    #[test]
    fn heading_is_identical_at_every_sample_when_more_packets_arrive() {
        let (_, full) = processed(turning_track(540));
        for count in (72..540).step_by(18) {
            let (_, prefix) = processed(turning_track(count));
            for i in 1..count {
                let t = i as f64 / 18.0;
                assert_eq!(
                    present(&prefix, Metric::Heading, t),
                    present(&full, Metric::Heading, t),
                    "prefix {count}, sample {i}"
                );
            }
        }
    }

    #[test]
    fn heading_recovers_after_a_gps_gap_without_connecting_across_it() {
        let mut track = turning_track(180);
        for p in &mut track[90..108] {
            p.lock = GpsLock::NoLock;
        }
        let (points, series) = processed(track);
        assert_eq!(present(&series, Metric::Heading, points[100].t), None);
        assert_eq!(present(&series, Metric::Heading, points[108].t), None);
        assert!(present(&series, Metric::Heading, points[109].t).is_some());
    }
}
