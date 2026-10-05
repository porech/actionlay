use actionlay_telemetry::{Metric, Telemetry};
pub fn telemetry() -> Telemetry {
    let data = [
        Metric::Speed,
        Metric::Alt,
        Metric::Gradient,
        Metric::Odo,
        Metric::Lat,
        Metric::Lon,
        Metric::Heading,
        Metric::Cog,
        Metric::Accel,
        Metric::AccelLon,
        Metric::AccelLat,
        Metric::Hr,
        Metric::Power,
        Metric::Cadence,
        Metric::GpsLock,
    ]
    .map(|m| {
        let points = (0..=1800)
            .map(|i| {
                let t = i as f64 / 18.0;
                let v = match m {
                    Metric::Speed => 9.0 + 2.0 * (t * 0.1).sin(),
                    Metric::Alt => 430.0 + 0.3 * t + 15.0 * (t * 0.08).sin(),
                    Metric::Gradient => 6.0 * (t * 0.08).cos(),
                    Metric::Odo => 9.0 * t,
                    Metric::Lat => 45.0 + 0.002 * (t * 0.02).sin(),
                    Metric::Lon => 9.0 + 0.002 * (1.0 - (t * 0.02).cos()),
                    Metric::Heading | Metric::Cog => (t * 1.15).rem_euclid(360.0),
                    Metric::Accel | Metric::AccelLon => 2.0 * (t * 0.3).cos(),
                    Metric::AccelLat => 3.0 * (t * 0.4).sin(),
                    Metric::Hr => 160.0 + 10.0 * (t * 0.1).sin(),
                    Metric::Power => 230.0 + 60.0 * (t * 0.15).sin(),
                    Metric::Cadence => 87.0,
                    Metric::GpsLock => 3.0,
                    _ => 0.0,
                };
                (t, v)
            })
            .collect();
        (m, points)
    });
    Telemetry::for_test(100.1, &data)
}
