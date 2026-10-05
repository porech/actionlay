//! Metric values to display: unit resolution and the empty-state policy (spec §4.4.1).
use actionlay_layout::model::WhenAbsent;
use actionlay_telemetry::Value;
use actionlay_telemetry::metric::Metric;
use actionlay_telemetry::units::{self, Unit, UnitSystem};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Shown {
    /// Drawn normally: a present value.
    Value(f64),
    /// The last value of a gap, drawn dimmed while the gap is within the grace time.
    Dimmed(f64),
    /// Designed empty state ("—" in the widget's style, dimmed).
    Empty,
    /// Not drawn at all.
    Hidden,
}

/// The empty-state policy (spec §4.4.1, rulings P1 and R6):
/// - `Present` → the value;
/// - `Stale` (a real gap: telemetry already bridges gaps up to 2 s as `Present`) →
///   its last value dimmed while `age <= grace_secs`, then the empty state; a grace of
///   0 gives the empty state at once;
/// - `Absent` → the empty state, or hidden when the widget says `hide` and the video
///   never has the metric (`available` false). A gap is never a reason to hide.
pub(crate) fn shown(
    value: Value,
    available: bool,
    grace_secs: f64,
    when_absent: WhenAbsent,
) -> Shown {
    match value {
        Value::Present(v) if v.is_finite() => Shown::Value(v),
        Value::Stale { value, age }
            if value.is_finite() && grace_secs > 0.0 && age <= grace_secs =>
        {
            Shown::Dimmed(value)
        }
        Value::Absent if !available && when_absent == WhenAbsent::Hide => Shown::Hidden,
        Value::Present(_) | Value::Stale { .. } | Value::Absent => Shown::Empty,
    }
}

/// [`shown`] for a widget whose metric id is unknown: never available, never a value.
pub(crate) fn shown_unknown(when_absent: WhenAbsent) -> Shown {
    shown(Value::Absent, false, 0.0, when_absent)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Resolved {
    pub metric: Metric,
    pub unit: Unit,
    /// Unit symbol ("" for dimensionless metrics).
    pub symbol: &'static str,
}

impl Resolved {
    /// Base-unit value → display unit.
    pub fn display(&self, base: f64) -> f64 {
        units::convert(base, self.unit)
    }
}

/// Metric and display unit of a widget: its own `units` if valid for the metric,
/// else the layout unit system's default. `None` for an unknown metric.
pub(crate) fn resolve(
    metric_id: &str,
    unit_id: Option<&str>,
    system: UnitSystem,
) -> Option<Resolved> {
    let metric = Metric::from_id(metric_id)?;
    let q = metric.quantity();
    let unit = unit_id
        .and_then(Unit::from_id)
        .filter(|u| units::units_for(q).contains(u))
        .unwrap_or_else(|| units::default_unit(q, system));
    Some(Resolved {
        metric,
        unit,
        symbol: units::symbol(unit),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_policy() {
        let (show, hide) = (WhenAbsent::Show, WhenAbsent::Hide);
        let stale = |value, age| Value::Stale { value, age };
        assert_eq!(
            shown(Value::Present(5.0), true, 3.0, show),
            Shown::Value(5.0)
        );
        assert_eq!(
            shown(Value::Present(f64::NAN), true, 3.0, show),
            Shown::Empty
        );
        // a gap shows the last value dimmed for the grace time, then the empty state
        assert_eq!(shown(stale(1.0, 2.0), true, 3.0, show), Shown::Dimmed(1.0));
        assert_eq!(shown(stale(1.0, 3.0), true, 3.0, show), Shown::Dimmed(1.0));
        assert_eq!(shown(stale(1.0, 3.5), true, 3.0, show), Shown::Empty);
        assert_eq!(shown(stale(f64::NAN, 0.5), true, 3.0, show), Shown::Empty);
        // grace 0: the empty state immediately (spec §4.4.1 alternative)
        assert_eq!(shown(stale(1.0, 0.1), true, 0.0, show), Shown::Empty);
        assert_eq!(shown(stale(1.0, 0.0), true, 0.0, show), Shown::Empty);
        // before the first sample
        assert_eq!(shown(Value::Absent, true, 3.0, show), Shown::Empty);
        assert_eq!(shown(Value::Absent, false, 3.0, show), Shown::Empty);
        // "hide" only when the video never has the metric (P1), not during a gap
        assert_eq!(shown(Value::Absent, false, 3.0, hide), Shown::Hidden);
        assert_eq!(shown(Value::Absent, true, 3.0, hide), Shown::Empty);
        assert_eq!(shown(stale(1.0, 9.0), true, 3.0, hide), Shown::Empty);
        assert_eq!(shown(stale(1.0, 1.0), true, 3.0, hide), Shown::Dimmed(1.0));
    }

    #[test]
    fn resolves_units_with_overrides_and_fallbacks() {
        let r = resolve("speed", None, UnitSystem::Metric).unwrap();
        assert_eq!((r.unit, r.symbol), (Unit::Kmh, "km/h"));
        assert!((r.display(10.0) - 36.0).abs() < 1e-9);
        assert_eq!(
            resolve("speed", None, UnitSystem::Imperial).unwrap().symbol,
            "mph"
        );
        assert_eq!(
            resolve("speed", Some("knots"), UnitSystem::Metric)
                .unwrap()
                .unit,
            Unit::Knots
        );
        // a unit of another quantity, or an unknown one, falls back to the default
        // (diagnose warns)
        for bad in ["ft", "furlong", "none"] {
            let r = resolve("speed", Some(bad), UnitSystem::Metric).unwrap();
            assert_eq!(r.symbol, "km/h", "{bad}");
        }
        let lat = resolve("lat", None, UnitSystem::Imperial).unwrap();
        assert_eq!((lat.unit, lat.symbol), (Unit::Deg, "°"));
        assert_eq!(lat.display(45.5), 45.5);
        // gradient is stored in percent (P2)
        let grad = resolve("gradient", None, UnitSystem::Metric).unwrap();
        assert_eq!((grad.symbol, grad.display(5.2)), ("%", 5.2));
        let hr = resolve("hr", None, UnitSystem::Metric).unwrap();
        assert_eq!((hr.unit, hr.symbol), (Unit::Plain, ""));
        assert_eq!(
            resolve("alt", Some("ft"), UnitSystem::Metric)
                .unwrap()
                .display(0.3048),
            1.0
        );
        assert!(resolve("heartbeat", None, UnitSystem::Metric).is_none());
    }
}
