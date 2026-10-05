//! Registry of the metrics a layout can show. String ids are those of
//! gopro-dashboard-overlay wherever it has the metric.
use crate::units::Quantity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Metric {
    Speed,
    CSpeed,
    Accel,
    Gradient,
    CGrad,
    Alt,
    Odo,
    COdo,
    Dist,
    Azi,
    Cog,
    Lat,
    Lon,
    GpsDop,
    GpsLock,
    AcclX,
    AcclY,
    AcclZ,
    GravX,
    GravY,
    GravZ,
    OriPitch,
    OriRoll,
    OriYaw,
    Temp,
    Hr,
    Cadence,
    Power,
    Respiration,
    GearFront,
    GearRear,
    Sdps,
}

impl Metric {
    /// Number of metrics.
    pub const COUNT: usize = 32;

    /// Every metric, in declaration order (`ALL[m.index()] == m`).
    pub const ALL: [Metric; Metric::COUNT] = [
        Metric::Speed,
        Metric::CSpeed,
        Metric::Accel,
        Metric::Gradient,
        Metric::CGrad,
        Metric::Alt,
        Metric::Odo,
        Metric::COdo,
        Metric::Dist,
        Metric::Azi,
        Metric::Cog,
        Metric::Lat,
        Metric::Lon,
        Metric::GpsDop,
        Metric::GpsLock,
        Metric::AcclX,
        Metric::AcclY,
        Metric::AcclZ,
        Metric::GravX,
        Metric::GravY,
        Metric::GravZ,
        Metric::OriPitch,
        Metric::OriRoll,
        Metric::OriYaw,
        Metric::Temp,
        Metric::Hr,
        Metric::Cadence,
        Metric::Power,
        Metric::Respiration,
        Metric::GearFront,
        Metric::GearRear,
        Metric::Sdps,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn id(self) -> &'static str {
        match self {
            Metric::Speed => "speed",
            Metric::CSpeed => "cspeed",
            Metric::Accel => "accel",
            Metric::Gradient => "gradient",
            Metric::CGrad => "cgrad",
            Metric::Alt => "alt",
            Metric::Odo => "odo",
            Metric::COdo => "codo",
            Metric::Dist => "dist",
            Metric::Azi => "azi",
            Metric::Cog => "cog",
            Metric::Lat => "lat",
            Metric::Lon => "lon",
            Metric::GpsDop => "gps-dop",
            Metric::GpsLock => "gps-lock",
            Metric::AcclX => "accl.x",
            Metric::AcclY => "accl.y",
            Metric::AcclZ => "accl.z",
            Metric::GravX => "grav.x",
            Metric::GravY => "grav.y",
            Metric::GravZ => "grav.z",
            Metric::OriPitch => "ori.pitch",
            Metric::OriRoll => "ori.roll",
            Metric::OriYaw => "ori.yaw",
            Metric::Temp => "temp",
            Metric::Hr => "hr",
            Metric::Cadence => "cadence",
            Metric::Power => "power",
            Metric::Respiration => "respiration",
            Metric::GearFront => "gear.front",
            Metric::GearRear => "gear.rear",
            Metric::Sdps => "sdps",
        }
    }

    pub fn from_id(id: &str) -> Option<Metric> {
        Metric::ALL.into_iter().find(|m| m.id() == id)
    }

    pub fn quantity(self) -> Quantity {
        use Metric::*;
        match self {
            Speed | CSpeed => Quantity::Speed,
            Accel | AcclX | AcclY | AcclZ => Quantity::Acceleration,
            Gradient | CGrad => Quantity::Ratio,
            Alt => Quantity::Altitude,
            Odo | COdo | Dist => Quantity::Distance,
            Azi | Cog | OriPitch | OriRoll | OriYaw => Quantity::Angle,
            Lat | Lon => Quantity::Coordinate,
            Temp => Quantity::Temperature,
            // Gravity is a unit vector in g, as in the original.
            GpsDop | GpsLock | GravX | GravY | GravZ => Quantity::Dimensionless,
            Hr | Cadence | Power | Respiration | GearFront | GearRear | Sdps => {
                Quantity::Dimensionless
            }
        }
    }

    /// True for metrics that only external files (GPX/FIT, M6) can provide.
    pub fn is_external(self) -> bool {
        use Metric::*;
        matches!(
            self,
            Hr | Cadence | Power | Respiration | GearFront | GearRear | Sdps
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_is_in_index_order_and_ids_round_trip() {
        for (i, m) in Metric::ALL.into_iter().enumerate() {
            assert_eq!(m.index(), i);
            assert_eq!(Metric::from_id(m.id()), Some(m));
        }
        assert_eq!(Metric::from_id("speed"), Some(Metric::Speed));
        assert_eq!(Metric::from_id("gps-lock"), Some(Metric::GpsLock));
        assert_eq!(Metric::from_id("accl.z"), Some(Metric::AcclZ));
        assert_eq!(Metric::from_id("nope"), None);
    }

    #[test]
    fn quantities() {
        assert_eq!(Metric::Speed.quantity(), Quantity::Speed);
        assert_eq!(Metric::CGrad.quantity(), Quantity::Ratio);
        assert_eq!(Metric::Alt.quantity(), Quantity::Altitude);
        assert_eq!(Metric::Odo.quantity(), Quantity::Distance);
        assert_eq!(Metric::Lat.quantity(), Quantity::Coordinate);
        assert_eq!(Metric::AcclX.quantity(), Quantity::Acceleration);
        assert_eq!(Metric::OriYaw.quantity(), Quantity::Angle);
        assert_eq!(Metric::Temp.quantity(), Quantity::Temperature);
        assert_eq!(Metric::GravZ.quantity(), Quantity::Dimensionless);
    }

    #[test]
    fn external_metrics() {
        assert!(Metric::Hr.is_external());
        assert!(!Metric::Speed.is_external());
        assert_eq!(Metric::ALL.iter().filter(|m| m.is_external()).count(), 7);
    }
}
