//! Physical quantities, display units and conversions.
//!
//! Values inside ActionLay are stored in *base units*: SI, except angles and
//! coordinates in degrees, temperature in °C, gradient in percent, and
//! dimensionless values as they are. [`convert`] turns a base value into a
//! display unit.

/// What a metric measures; decides which units can display it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quantity {
    Speed,
    Distance,
    Altitude,
    Acceleration,
    Angle,
    Temperature,
    /// Gradient, in percent.
    Ratio,
    Dimensionless,
    Coordinate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitSystem {
    Metric,
    Imperial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    Kmh,
    Mph,
    Knots,
    Mps,
    PaceKm,
    PaceMile,
    PaceNm,
    Km,
    Mi,
    Nmi,
    M,
    Ft,
    G,
    Mps2,
    DegC,
    DegF,
    Deg,
    Percent,
    /// Dimensionless values (DOP, lock state, gravity in g).
    None,
}

const ALL_UNITS: [Unit; 19] = [
    Unit::Kmh,
    Unit::Mph,
    Unit::Knots,
    Unit::Mps,
    Unit::PaceKm,
    Unit::PaceMile,
    Unit::PaceNm,
    Unit::Km,
    Unit::Mi,
    Unit::Nmi,
    Unit::M,
    Unit::Ft,
    Unit::G,
    Unit::Mps2,
    Unit::DegC,
    Unit::DegF,
    Unit::Deg,
    Unit::Percent,
    Unit::None,
];

/// Standard gravity, m/s².
pub const STANDARD_GRAVITY: f64 = 9.806_65;
const MILE_M: f64 = 1609.344;
const NAUTICAL_MILE_M: f64 = 1852.0;
const FOOT_M: f64 = 0.3048;

impl Unit {
    pub fn id(self) -> &'static str {
        match self {
            Unit::Kmh => "kmh",
            Unit::Mph => "mph",
            Unit::Knots => "knots",
            Unit::Mps => "mps",
            Unit::PaceKm => "pace_km",
            Unit::PaceMile => "pace_mile",
            Unit::PaceNm => "pace_nm",
            Unit::Km => "km",
            Unit::Mi => "mi",
            Unit::Nmi => "nmi",
            Unit::M => "m",
            Unit::Ft => "ft",
            Unit::G => "g",
            Unit::Mps2 => "mps2",
            Unit::DegC => "degc",
            Unit::DegF => "degf",
            Unit::Deg => "deg",
            Unit::Percent => "percent",
            Unit::None => "none",
        }
    }

    pub fn from_id(id: &str) -> Option<Unit> {
        ALL_UNITS.into_iter().find(|u| u.id() == id)
    }
}

/// Units that can display `q`, default units first.
pub fn units_for(q: Quantity) -> &'static [Unit] {
    match q {
        Quantity::Speed => &[
            Unit::Kmh,
            Unit::Mph,
            Unit::Knots,
            Unit::Mps,
            Unit::PaceKm,
            Unit::PaceMile,
            Unit::PaceNm,
        ],
        Quantity::Distance => &[Unit::Km, Unit::Mi, Unit::Nmi, Unit::M],
        Quantity::Altitude => &[Unit::M, Unit::Ft],
        Quantity::Acceleration => &[Unit::Mps2, Unit::G],
        Quantity::Temperature => &[Unit::DegC, Unit::DegF],
        Quantity::Angle | Quantity::Coordinate => &[Unit::Deg],
        Quantity::Ratio => &[Unit::Percent],
        Quantity::Dimensionless => &[Unit::None],
    }
}

pub fn default_unit(q: Quantity, system: UnitSystem) -> Unit {
    let imperial = system == UnitSystem::Imperial;
    match q {
        Quantity::Speed if imperial => Unit::Mph,
        Quantity::Speed => Unit::Kmh,
        Quantity::Distance if imperial => Unit::Mi,
        Quantity::Distance => Unit::Km,
        Quantity::Altitude if imperial => Unit::Ft,
        Quantity::Altitude => Unit::M,
        Quantity::Temperature if imperial => Unit::DegF,
        Quantity::Temperature => Unit::DegC,
        Quantity::Acceleration => Unit::Mps2,
        Quantity::Angle | Quantity::Coordinate => Unit::Deg,
        Quantity::Ratio => Unit::Percent,
        Quantity::Dimensionless => Unit::None,
    }
}

/// Converts a value in base units to `unit`. Pace of a standstill is +∞.
pub fn convert(si: f64, unit: Unit) -> f64 {
    let pace = |metres: f64| {
        if si > 0.0 {
            metres / 60.0 / si
        } else {
            f64::INFINITY
        }
    };
    match unit {
        Unit::Kmh => si * 3.6,
        Unit::Mph => si * 3600.0 / MILE_M,
        Unit::Knots => si * 3600.0 / NAUTICAL_MILE_M,
        Unit::PaceKm => pace(1000.0),
        Unit::PaceMile => pace(MILE_M),
        Unit::PaceNm => pace(NAUTICAL_MILE_M),
        Unit::Km => si / 1000.0,
        Unit::Mi => si / MILE_M,
        Unit::Nmi => si / NAUTICAL_MILE_M,
        Unit::Ft => si / FOOT_M,
        Unit::G => si / STANDARD_GRAVITY,
        Unit::DegF => si * 9.0 / 5.0 + 32.0,
        Unit::Mps | Unit::M | Unit::Mps2 | Unit::DegC | Unit::Deg | Unit::Percent | Unit::None => {
            si
        }
    }
}

pub fn symbol(unit: Unit) -> &'static str {
    match unit {
        Unit::Kmh => "km/h",
        Unit::Mph => "mph",
        Unit::Knots => "kn",
        Unit::Mps => "m/s",
        Unit::PaceKm => "min/km",
        Unit::PaceMile => "min/mi",
        Unit::PaceNm => "min/nmi",
        Unit::Km => "km",
        Unit::Mi => "mi",
        Unit::Nmi => "nmi",
        Unit::M => "m",
        Unit::Ft => "ft",
        Unit::G => "G",
        Unit::Mps2 => "m/s²",
        Unit::DegC => "°C",
        Unit::DegF => "°F",
        Unit::Deg => "°",
        Unit::Percent => "%",
        Unit::None => "",
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 * b.abs().max(1.0)
    }

    #[test]
    fn converts_speed() {
        assert!(close(convert(10.0, Unit::Kmh), 36.0));
        assert!(close(convert(10.0, Unit::Mph), 22.369_362_920_544_02));
        assert!(close(convert(10.0, Unit::Knots), 19.438_444_924_406_05));
        assert!(close(convert(10.0, Unit::Mps), 10.0));
    }

    #[test]
    fn converts_pace_and_standstill() {
        // 10 km/h = 6 min/km
        assert!(close(convert(10.0 / 3.6, Unit::PaceKm), 6.0));
        assert!(close(convert(10.0 / 3.6, Unit::PaceMile), 9.656_064));
        assert!(convert(0.0, Unit::PaceKm).is_infinite());
    }

    #[test]
    fn converts_distance_altitude_temperature_acceleration() {
        assert!(close(convert(1609.344, Unit::Mi), 1.0));
        assert!(close(convert(1852.0, Unit::Nmi), 1.0));
        assert!(close(convert(2500.0, Unit::Km), 2.5));
        assert!(close(convert(100.0, Unit::Ft), 328.083_989_501_312_3));
        assert!(close(convert(100.0, Unit::DegF), 212.0));
        assert!(close(convert(-40.0, Unit::DegF), -40.0));
        assert!(close(convert(9.806_65, Unit::G), 1.0));
    }

    #[test]
    fn ids_round_trip_and_are_unique() {
        for u in ALL_UNITS {
            assert_eq!(Unit::from_id(u.id()), Some(u));
        }
        assert_eq!(Unit::from_id("furlong"), None);
    }

    #[test]
    fn defaults_per_system() {
        assert_eq!(default_unit(Quantity::Speed, UnitSystem::Metric), Unit::Kmh);
        assert_eq!(
            default_unit(Quantity::Speed, UnitSystem::Imperial),
            Unit::Mph
        );
        assert_eq!(
            default_unit(Quantity::Altitude, UnitSystem::Imperial),
            Unit::Ft
        );
        assert_eq!(
            default_unit(Quantity::Distance, UnitSystem::Imperial),
            Unit::Mi
        );
        assert_eq!(
            default_unit(Quantity::Temperature, UnitSystem::Imperial),
            Unit::DegF
        );
        assert_eq!(
            default_unit(Quantity::Acceleration, UnitSystem::Metric),
            Unit::Mps2
        );
        assert_eq!(
            default_unit(Quantity::Ratio, UnitSystem::Metric),
            Unit::Percent
        );
        for q in [
            Quantity::Speed,
            Quantity::Distance,
            Quantity::Altitude,
            Quantity::Acceleration,
            Quantity::Angle,
            Quantity::Temperature,
            Quantity::Ratio,
            Quantity::Dimensionless,
            Quantity::Coordinate,
        ] {
            for s in [UnitSystem::Metric, UnitSystem::Imperial] {
                assert!(units_for(q).contains(&default_unit(q, s)), "{q:?} {s:?}");
            }
        }
    }

    #[test]
    fn symbols() {
        assert_eq!(symbol(Unit::Kmh), "km/h");
        assert_eq!(symbol(Unit::DegC), "°C");
        assert_eq!(symbol(Unit::G), "G");
        assert_eq!(symbol(Unit::Percent), "%");
        assert_eq!(symbol(Unit::PaceKm), "min/km");
    }
}
