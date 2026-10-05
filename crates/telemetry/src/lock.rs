//! GPS lock filter, after gopro-dashboard-overlay's `gpmd_filters.standard`.
use crate::{extract::GpsPoint, value::GpsLock};

/// When a recorded fix is downgraded to "no lock".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LockOptions {
    /// Points with a DOP above this are not locked (original default: 10).
    pub dop_max: f64,
    /// Points faster than this (m/s) are not locked. The original defaults
    /// to 60 km/h, which would blank every car or motorbike video, so
    /// ActionLay leaves it off by default.
    pub speed_max: Option<f64>,
}

impl Default for LockOptions {
    fn default() -> Self {
        LockOptions {
            dop_max: 10.0,
            speed_max: None,
        }
    }
}

/// Sets `lock` on every point. Besides the DOP and speed limits it keeps the
/// original's heuristic: a point that claims a fix right after a point
/// without one, but repeats that point's position or speed, is a stale
/// reading and keeps the previous (unlocked) state.
pub(crate) fn apply(points: &mut [GpsPoint], opts: &LockOptions) {
    // The last point the heuristic accepted: (lock, lat, lon, speed2d).
    let mut last: Option<(GpsLock, f64, f64, f64)> = None;
    for p in points.iter_mut() {
        let recorded = GpsLock::from_fix(p.fix);
        let mut lock = recorded;
        match last {
            Some((prev, lat, lon, speed))
                if recorded.is_locked()
                    && !prev.is_locked()
                    && ((p.lat == lat && p.lon == lon) || p.speed2d == speed) =>
            {
                lock = prev;
            }
            _ => last = Some((recorded, p.lat, p.lon, p.speed2d)),
        }
        // Written as `!(x <= max)` so that a NaN DOP or speed (damaged
        // sample) counts as unlocked, whatever the limits.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        let bad = !(p.dop <= opts.dop_max)
            || p.speed2d.is_nan()
            || opts.speed_max.is_some_and(|max| !(p.speed2d <= max));
        if bad {
            lock = GpsLock::NoLock;
        }
        p.lock = lock;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::Derived;

    pub(crate) fn point(fix: u32, dop: f64, lat: f64, speed: f64) -> GpsPoint {
        GpsPoint {
            packet: 0,
            index: 0,
            t: 0.0,
            end: 0.0,
            utc: None,
            lat,
            lon: 7.0,
            alt: 100.0,
            speed2d: speed,
            speed3d: speed,
            fix,
            dop,
            lock: GpsLock::from_fix(fix),
            derived: Derived::default(),
        }
    }

    fn locks(points: &mut [GpsPoint], opts: LockOptions) -> Vec<GpsLock> {
        apply(points, &opts);
        points.iter().map(|p| p.lock).collect()
    }

    #[test]
    fn dop_above_limit_is_not_locked() {
        let mut pts = [
            point(3, 10.0, 45.0, 1.0),
            point(3, 10.01, 45.1, 2.0),
            point(2, 3.0, 45.2, 3.0),
        ];
        assert_eq!(
            locks(&mut pts, LockOptions::default()),
            vec![GpsLock::Lock3d, GpsLock::NoLock, GpsLock::Lock2d]
        );
    }

    #[test]
    fn speed_limit_is_optional() {
        let mut pts = [point(3, 1.0, 45.0, 20.0)];
        assert_eq!(
            locks(&mut pts, LockOptions::default()),
            vec![GpsLock::Lock3d]
        );
        let opts = LockOptions {
            speed_max: Some(60.0 / 3.6),
            ..LockOptions::default()
        };
        assert_eq!(locks(&mut pts, opts), vec![GpsLock::NoLock]);
    }

    #[test]
    fn nan_dop_or_speed_is_not_locked() {
        let mut pts = [
            point(3, f64::NAN, 45.0, 1.0),
            point(3, 1.0, 45.1, f64::NAN),
            point(3, 1.0, 45.2, 2.0),
        ];
        assert_eq!(
            locks(&mut pts, LockOptions::default()),
            vec![GpsLock::NoLock, GpsLock::NoLock, GpsLock::Lock3d]
        );
        let opts = LockOptions {
            speed_max: Some(10.0),
            ..LockOptions::default()
        };
        assert_eq!(
            locks(&mut pts, opts),
            vec![GpsLock::NoLock, GpsLock::NoLock, GpsLock::Lock3d]
        );
    }

    #[test]
    fn fix_repeating_the_unlocked_reading_is_ignored() {
        let mut pts = [
            point(0, 99.99, 45.0, 0.0),
            point(3, 2.0, 45.0, 1.0), // same position as the unlocked point
            point(3, 2.0, 45.1, 0.0), // same speed as the unlocked point
            point(3, 2.0, 45.2, 2.0), // genuinely new
            point(3, 2.0, 45.2, 2.0), // repeats, but the previous was locked
        ];
        assert_eq!(
            locks(&mut pts, LockOptions::default()),
            vec![
                GpsLock::NoLock,
                GpsLock::NoLock,
                GpsLock::NoLock,
                GpsLock::Lock3d,
                GpsLock::Lock3d
            ]
        );
    }
}
