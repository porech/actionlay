//! What a metric reads at one instant.

/// A metric's reading at time t, in base units (see [`crate::units`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    /// Valid data at t.
    Present(f64),
    /// Temporary gap: the last valid value and the seconds since it stopped
    /// being valid.
    Stale { value: f64, age: f64 },
    /// Nothing to show at t: the file has no valid sample of this metric at
    /// or before t. Whether the metric exists in the file at all is
    /// `Availability::coverage(m) > 0`.
    Absent,
}

impl Value {
    /// The value if Present.
    pub fn present(self) -> Option<f64> {
        match self {
            Value::Present(v) => Some(v),
            _ => None,
        }
    }

    /// The value if Present or Stale.
    pub fn last_known(self) -> Option<f64> {
        match self {
            Value::Present(v) | Value::Stale { value: v, .. } => Some(v),
            Value::Absent => None,
        }
    }
}

/// GPS fix after ActionLay's lock filter (DOP, heuristics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpsLock {
    NoLock,
    Lock2d,
    Lock3d,
    /// No GPS stream, or a fix code the camera did not document.
    Unknown,
}

impl GpsLock {
    /// GPSF codes: 0 = no lock, 2 = 2D, 3 = 3D.
    pub fn from_fix(fix: u32) -> GpsLock {
        match fix {
            0 => GpsLock::NoLock,
            2 => GpsLock::Lock2d,
            3 => GpsLock::Lock3d,
            _ => GpsLock::Unknown,
        }
    }

    pub fn is_locked(self) -> bool {
        matches!(self, GpsLock::Lock2d | GpsLock::Lock3d)
    }

    /// The GPSF code (Unknown → 1, as gopro-dashboard-overlay's GPSFix).
    pub fn code(self) -> u32 {
        match self {
            GpsLock::NoLock => 0,
            GpsLock::Unknown => 1,
            GpsLock::Lock2d => 2,
            GpsLock::Lock3d => 3,
        }
    }

    /// Name used by gopro-to-csv's `gps_fix` column.
    pub fn original_name(self) -> &'static str {
        match self {
            GpsLock::NoLock => "NO",
            GpsLock::Unknown => "UNKNOWN",
            GpsLock::Lock2d => "LOCK_2D",
            GpsLock::Lock3d => "LOCK_3D",
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_accessors() {
        assert_eq!(Value::Present(1.0).present(), Some(1.0));
        assert_eq!(
            Value::Stale {
                value: 2.0,
                age: 1.0
            }
            .present(),
            None
        );
        assert_eq!(
            Value::Stale {
                value: 2.0,
                age: 1.0
            }
            .last_known(),
            Some(2.0)
        );
        assert_eq!(Value::Absent.last_known(), None);
    }

    #[test]
    fn lock_codes_round_trip() {
        for l in [
            GpsLock::NoLock,
            GpsLock::Lock2d,
            GpsLock::Lock3d,
            GpsLock::Unknown,
        ] {
            assert_eq!(GpsLock::from_fix(l.code()), l);
        }
        assert_eq!(GpsLock::from_fix(7), GpsLock::Unknown);
        assert!(GpsLock::Lock2d.is_locked() && !GpsLock::NoLock.is_locked());
        assert_eq!(GpsLock::Lock3d.original_name(), "LOCK_3D");
    }
}
