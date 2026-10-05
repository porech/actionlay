//! Metrics derived from the GPS track, computed as gopro-dashboard-overlay
//! 0.134.0 computes them for a dashboard (`gopro-dashboard.py`, the
//! "processing" block; `timeseries_process.py`; `framemeta.py`).
//!
//! Points are visited in time order; "locked" means a 2D or 3D fix after the
//! lock filter. With S = 54 (3 s at 18 Hz):
//!
//! 1. **lat/lon** of locked points are smoothed with [`Ses`] (α = 0.45).
//! 2. For each pair (i, i+S) with both points locked, on the smoothed
//!    positions: `d` = WGS84 geodesic distance, `azi` = initial bearing
//!    (−180…180°), `dt` = UTC difference (file time if a UTC is missing),
//!    `cspeed` = Kalman(d / dt, or 0 when dt ≤ 0), `dist` = d / S,
//!    `cog` = azi mod 360. Pairs are stored on point i for i < n−S; then the
//!    last S points get the pair (j−S, j) stored on j, continuing the same
//!    Kalman state.
//! 3. **codo** = running sum of `dist` over locked points.
//! 4. **accel** on point i+S for every pair (i, i+S), locked or not:
//!    `(v[i+S] − v[i]) / dt` on the recorded 2D speed, or 0 when either
//!    speed or dt is 0.
//! 5. **cgrad** for pairs with both points 3D-locked and both altitudes
//!    non-zero: `100·Δalt / d` when d > 1 m and |result| < 45, stored like
//!    step 2.
//! 6. **speed** = Kalman over the recorded 2D speed of every point.
//! 7. Every derived value of a point that is not locked is cleared.
//!
//! The Kalman and SES filters never recover from a NaN or infinity, so
//! non-finite inputs never reach them: a point whose latitude or longitude is
//! not finite is treated as not locked, a non-finite altitude only drops that
//! point's `alt` and gradients, and a non-finite recorded speed only drops
//! that point's `speed` and `accel`.
//!
//! With n ≤ S points there are no pairs and nothing but lat/lon/alt/speed/codo
//! is derived. With S < n < 2S the backward pass only covers points j ≥ S.
//! (The original wraps around to the end of the track with negative indices
//! in both cases; recordings shorter than ~6 s of GPS are affected.)
use geographiclib_rs::{Geodesic, InverseGeodesic};

use crate::{
    extract::{Derived, GpsPoint},
    smoothing::{Kalman, Ses},
    value::GpsLock,
};

pub(crate) const PAIR_SKIP: usize = 54;
const SES_ALPHA: f64 = 0.45;
const CGRAD_MIN_DIST: f64 = 1.0;
const CGRAD_MAX_ABS: f64 = 45.0;

/// A locked point whose position is usable (finite and on the globe; the
/// range checks are false for NaN).
fn locked(p: &GpsPoint) -> bool {
    p.lock.is_locked() && p.lat.abs() <= 90.0 && p.lon.abs() <= 180.0
}

fn locked_3d(p: &GpsPoint) -> bool {
    locked(p) && p.lock == GpsLock::Lock3d && p.alt.is_finite()
}

fn dt(a: &GpsPoint, b: &GpsPoint) -> f64 {
    match (a.utc, b.utc) {
        (Some(ua), Some(ub)) => (ub - ua).as_seconds_f64(),
        _ => b.t - a.t,
    }
}

/// Distance (m) and initial bearing (°) between two smoothed positions.
fn inverse(a: &[f64; 2], b: &[f64; 2]) -> (f64, f64) {
    let (s12, azi1, _azi2, _a12): (f64, f64, f64, f64) =
        Geodesic::wgs84().inverse(a[0], a[1], b[0], b[1]);
    (s12, azi1)
}

pub(crate) fn derive(points: &mut [GpsPoint]) {
    let n = points.len();
    for p in points.iter_mut() {
        p.derived = Derived::default();
    }

    // 1. smoothed positions (raw for points that are not locked)
    let mut ses = Ses::new(SES_ALPHA);
    let pos: Vec<[f64; 2]> = points
        .iter()
        .map(|p| {
            if locked(p) {
                ses.update([p.lat, p.lon])
            } else {
                [p.lat, p.lon]
            }
        })
        .collect();

    // the forward pairs, then the last S points paired backwards
    let pairs: Vec<(usize, usize, usize)> = if n > PAIR_SKIP {
        (0..n - PAIR_SKIP)
            .map(|i| (i, i + PAIR_SKIP, i))
            .chain(((n - PAIR_SKIP).max(PAIR_SKIP)..n).map(|j| (j - PAIR_SKIP, j, j)))
            .collect()
    } else {
        Vec::new()
    };

    // 2. speeds, distance, bearing
    let mut kalman = Kalman::default();
    for &(a, b, store) in &pairs {
        if !(locked(&points[a]) && locked(&points[b])) {
            continue;
        }
        let (d, azi) = inverse(&pos[a], &pos[b]);
        let dt = dt(&points[a], &points[b]);
        let raw = if dt > 0.0 { d / dt } else { 0.0 };
        if !(raw.is_finite() && d.is_finite() && azi.is_finite()) {
            continue;
        }
        let out = &mut points[store].derived;
        out.cspeed = Some(kalman.update(raw));
        out.dist = Some(d / PAIR_SKIP as f64);
        out.azi = Some(azi);
        out.cog = Some(if azi >= 0.0 { azi + 0.0 } else { azi + 360.0 });
    }

    // 3. odometer
    let mut total = 0.0;
    for p in points.iter_mut().filter(|p| locked(p)) {
        total += p.derived.dist.unwrap_or(0.0);
        p.derived.codo = Some(total);
    }

    // 4. acceleration from the recorded speed (forward pairs only)
    for i in 0..n.saturating_sub(PAIR_SKIP) {
        let (a, b) = (&points[i], &points[i + PAIR_SKIP]);
        let dt = dt(a, b);
        if !(a.speed2d.is_finite() && b.speed2d.is_finite() && dt.is_finite()) {
            continue;
        }
        let accel = if a.speed2d != 0.0 && b.speed2d != 0.0 && dt != 0.0 {
            (b.speed2d - a.speed2d) / dt
        } else {
            0.0
        };
        points[i + PAIR_SKIP].derived.accel = Some(accel);
    }

    // 5. gradient
    for &(a, b, store) in &pairs {
        let (pa, pb) = (&points[a], &points[b]);
        if !(locked_3d(pa) && locked_3d(pb)) || pa.alt == 0.0 || pb.alt == 0.0 {
            continue;
        }
        let gain = pb.alt - pa.alt;
        let (d, _) = inverse(&pos[a], &pos[b]);
        if d.is_finite() && d > CGRAD_MIN_DIST {
            let grad = gain / d * 100.0;
            if grad.abs() < CGRAD_MAX_ABS {
                points[store].derived.cgrad = Some(grad);
            }
        }
    }

    // 6. smoothed speed, 7. clear what is not locked
    let mut kalman = Kalman::default();
    for (p, smoothed) in points.iter_mut().zip(&pos) {
        let speed = p.speed2d.is_finite().then(|| kalman.update(p.speed2d));
        if locked(p) {
            let d = &mut p.derived;
            d.lat = Some(smoothed[0]);
            d.lon = Some(smoothed[1]);
            d.alt = p.alt.is_finite().then_some(p.alt);
            d.speed = speed;
        } else {
            p.derived = Derived::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeDelta, Utc};

    /// 1 m in latitude at 45° N (WGS84).
    const DEG_PER_M: f64 = 1.0 / 111_131.745;
    const RATE: f64 = 18.0;

    fn utc0() -> DateTime<Utc> {
        "2024-05-01T10:00:00Z".parse().unwrap()
    }

    /// n points heading north at `speed` m/s, climbing `climb` m per second.
    fn track(n: usize, speed: f64, climb: f64) -> Vec<GpsPoint> {
        (0..n)
            .map(|i| {
                let t = i as f64 / RATE;
                GpsPoint {
                    packet: i / 18,
                    index: i % 18,
                    t,
                    end: t + 1.0 / RATE,
                    utc: Some(utc0() + TimeDelta::microseconds((t * 1e6).round() as i64)),
                    lat: 45.0 + speed * t * DEG_PER_M,
                    lon: 7.0,
                    alt: 100.0 + climb * t,
                    speed2d: speed,
                    speed3d: speed,
                    fix: 3,
                    dop: 1.0,
                    lock: GpsLock::Lock3d,
                    derived: Derived::default(),
                }
            })
            .collect()
    }

    #[test]
    fn steady_northbound_track() {
        let mut pts = track(200, 1.0, 0.1);
        derive(&mut pts);
        let d = pts[100].derived;
        assert!((d.cspeed.unwrap() - 1.0).abs() < 0.01, "{d:?}");
        assert!((d.dist.unwrap() - 1.0 / RATE).abs() < 0.001, "{d:?}");
        assert!(d.azi.unwrap().abs() < 1e-6, "{d:?}");
        let cog = d.cog.unwrap();
        assert!(cog < 1e-6 || cog > 360.0 - 1e-6, "{d:?}");
        // climb 0.1 m/s at 1 m/s → 10 %
        assert!((d.cgrad.unwrap() - 10.0).abs() < 0.1, "{d:?}");
        assert_eq!(d.accel, Some(0.0));
        assert_eq!(d.speed, Some(1.0));
        assert_eq!(d.alt, Some(pts[100].alt));
        // smoothing lags the raw position by about 2.2 samples
        let lag_m = (pts[100].lat - d.lat.unwrap()) / DEG_PER_M;
        assert!((lag_m - 2.22 / RATE).abs() < 0.01, "lag {lag_m}");
    }

    #[test]
    fn every_point_of_a_long_locked_track_gets_values() {
        let mut pts = track(200, 1.0, 0.1);
        derive(&mut pts);
        for (i, p) in pts.iter().enumerate() {
            let d = p.derived;
            assert!(
                d.cspeed.is_some() && d.dist.is_some() && d.cgrad.is_some(),
                "point {i}"
            );
            assert_eq!(d.accel.is_some(), i >= PAIR_SKIP, "point {i}");
        }
    }

    #[test]
    fn odometer_sums_dist() {
        let mut pts = track(200, 2.0, 0.0);
        derive(&mut pts);
        let mut sum = 0.0;
        for p in &pts {
            sum += p.derived.dist.unwrap();
            assert_eq!(p.derived.codo, Some(sum));
        }
        // ~ 2 m/s × 199/18 s, a little less while the smoothing settles
        let codo = pts.last().unwrap().derived.codo.unwrap();
        assert!((codo - 2.0 * 199.0 / RATE).abs() < 0.3, "codo {codo}");
    }

    #[test]
    fn accel_uses_recorded_speed() {
        let mut pts = track(120, 1.0, 0.0);
        for (i, p) in pts.iter_mut().enumerate() {
            p.speed2d = 1.0 + i as f64 / RATE; // +1 m/s per second
        }
        derive(&mut pts);
        assert_eq!(pts[10].derived.accel, None);
        assert!((pts[60].derived.accel.unwrap() - 1.0).abs() < 1e-6);
        // a standstill at either end gives 0, as in the original
        pts[0].speed2d = 0.0;
        derive(&mut pts);
        assert_eq!(pts[PAIR_SKIP].derived.accel, Some(0.0));
    }

    #[test]
    fn steep_or_short_gradients_are_dropped() {
        let mut pts = track(120, 1.0, 0.5); // 50 %: rejected
        derive(&mut pts);
        assert!(pts.iter().all(|p| p.derived.cgrad.is_none()));
        let mut pts = track(120, 0.2, 0.01); // 0.6 m per pair: too short
        derive(&mut pts);
        assert!(pts.iter().all(|p| p.derived.cgrad.is_none()));
    }

    #[test]
    fn unlocked_points_have_no_derived_values_and_odo_continues() {
        let mut pts = track(200, 1.0, 0.0);
        for p in &mut pts[60..80] {
            p.lock = GpsLock::NoLock;
        }
        derive(&mut pts);
        for p in &pts[60..80] {
            assert_eq!(p.derived, Derived::default());
        }
        // pairs that reach into the gap are skipped …
        assert_eq!(pts[10].derived.cspeed, None);
        // … pairs that jump over it are kept, as in the original
        assert!(pts[40].derived.cspeed.is_some());
        let before = pts[59].derived.codo.unwrap();
        let after = pts[80].derived.codo.unwrap();
        assert!(after >= before);
    }

    #[test]
    fn two_d_points_get_speed_but_no_gradient() {
        let mut pts = track(200, 1.0, 0.1);
        for p in &mut pts {
            p.lock = GpsLock::Lock2d;
        }
        derive(&mut pts);
        assert!(pts[100].derived.cspeed.is_some());
        assert!(pts.iter().all(|p| p.derived.cgrad.is_none()));
    }

    #[test]
    fn tracks_shorter_than_two_windows_do_not_wrap_around() {
        let mut pts = track(80, 1.0, 0.0);
        derive(&mut pts);
        // forward pairs for i < 26, backward pairs for j ≥ 54
        assert!(pts[25].derived.cspeed.is_some());
        assert!(pts[26..54].iter().all(|p| p.derived.cspeed.is_none()));
        assert!(pts[54].derived.cspeed.is_some());
    }

    #[test]
    fn short_tracks_have_no_pairs() {
        let mut pts = track(PAIR_SKIP, 1.0, 0.0);
        derive(&mut pts);
        for p in &pts {
            let d = p.derived;
            assert!(d.lat.is_some() && d.speed.is_some() && d.codo == Some(0.0));
            assert!(d.cspeed.is_none() && d.dist.is_none() && d.accel.is_none());
        }
        derive(&mut []);
    }

    #[test]
    fn cog_of_a_due_north_bearing_is_positive_zero() {
        let mut pts = track(120, 1.0, 0.0);
        derive(&mut pts);
        for p in &pts {
            assert!(p.derived.cog.unwrap().is_sign_positive() || p.derived.cog.unwrap() > 359.0);
        }
    }

    #[test]
    fn non_finite_inputs_do_not_poison_the_series() {
        for field in 0..5 {
            let mut pts = track(300, 1.0, 0.1);
            match field {
                0 => pts[100].alt = f64::NAN,
                1 => pts[100].speed2d = f64::NAN,
                2 => pts[100].lat = f64::NAN,
                4 => pts[100].lat = 214.0,
                _ => pts[100].speed2d = f64::INFINITY,
            }
            derive(&mut pts);
            for (i, p) in pts.iter().enumerate() {
                let d = p.derived;
                for (name, v) in [
                    ("lat", d.lat),
                    ("lon", d.lon),
                    ("alt", d.alt),
                    ("speed", d.speed),
                    ("cspeed", d.cspeed),
                    ("dist", d.dist),
                    ("codo", d.codo),
                    ("azi", d.azi),
                    ("cog", d.cog),
                    ("cgrad", d.cgrad),
                    ("accel", d.accel),
                ] {
                    assert!(
                        v.is_none_or(f64::is_finite),
                        "case {field} point {i} {name}"
                    );
                }
            }
            for (i, p) in pts.iter().enumerate().skip(160) {
                let d = p.derived;
                assert!(
                    d.speed.is_some() && d.cspeed.is_some(),
                    "case {field} point {i}"
                );
                assert!(
                    d.lat.is_some() && d.codo.is_some(),
                    "case {field} point {i}"
                );
            }
            assert!((pts[299].derived.cspeed.unwrap() - 1.0).abs() < 0.05);
            // neighbours of the bad point (pairs touching 100: stored on 46 and 100)
            let d = |i: usize| pts[i].derived;
            match field {
                0 => {
                    assert_eq!(d(100).alt, None);
                    assert!(d(100).speed.is_some() && d(100).cspeed.is_some());
                    assert!(d(46).cgrad.is_none() && d(100).cgrad.is_none());
                    for i in [45, 47, 99, 101, 102] {
                        assert!(d(i).cgrad.is_some(), "cgrad {i}");
                    }
                }
                1 | 3 => {
                    assert_eq!(d(100).speed, None);
                    assert_eq!(d(100).accel, None);
                    assert!(d(101).speed.is_some());
                    assert!(d(100).cspeed.is_some() && d(100).lat.is_some());
                }
                _ => {
                    // not locked: everything on the point is cleared
                    assert_eq!(d(100), Derived::default(), "case {field}");
                    assert!(d(46).cspeed.is_none() && d(46).cgrad.is_none());
                    for i in [45, 47, 99, 101] {
                        assert!(d(i).cspeed.is_some(), "cspeed {i}");
                    }
                    assert!(d(101).speed.is_some() && d(101).lat.is_some());
                }
            }
        }
    }
}
