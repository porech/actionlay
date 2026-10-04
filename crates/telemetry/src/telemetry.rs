use chrono::{DateTime, TimeDelta, Utc};

use crate::{
    RawPacket,
    derive::derive,
    extract::{Extracted, GpsPoint, Sample, extract},
    gpmf::GpmfError,
    lock::{self, LockOptions},
    metric::Metric,
    series::{Interp, Series, gaps_and_coverage},
    smoothing::Kalman,
    value::{GpsLock, Value},
};

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("none of the {packets} GPMF packets could be read: {first}")]
    Unreadable { packets: usize, first: GpmfError },
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TelemetryOptions {
    pub lock: LockOptions,
}

/// A locked GPS point as displayed (smoothed position), for maps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPoint {
    pub t: f64,
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
}

/// Which metrics a video has, and where.
#[derive(Debug, Clone)]
pub struct Availability {
    /// Indexed by `Metric::index()`: (coverage, gaps).
    per_metric: Vec<(f64, Vec<(f64, f64)>)>,
}

impl Availability {
    /// Share of `[0, duration]` where the metric is Present, 0.0..=1.0.
    pub fn coverage(&self, m: Metric) -> f64 {
        self.per_metric[m.index()].0
    }

    /// Intervals of `[0, duration]` where the metric is not Present.
    pub fn gaps(&self, m: Metric) -> &[(f64, f64)] {
        &self.per_metric[m.index()].1
    }

    /// True when the video has the metric anywhere.
    pub fn is_available(&self, m: Metric) -> bool {
        self.coverage(m) > 0.0
    }
}

/// Every metric at one instant.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub t: f64,
    pub utc: Option<DateTime<Utc>>,
    pub gps_lock: GpsLock,
    /// Indexed by `Metric::index()`.
    values: [Value; Metric::COUNT],
    /// Indexed by `Metric::index()`: the video has the metric somewhere.
    available: [bool; Metric::COUNT],
}

impl Snapshot {
    pub fn get(&self, m: Metric) -> Value {
        self.values[m.index()]
    }

    /// True when the metric has coverage > 0 anywhere in this video (same as
    /// `Availability::is_available`), so a renderer can hide a widget whose
    /// metric the video never has.
    pub fn is_available(&self, m: Metric) -> bool {
        self.available[m.index()]
    }
}

#[derive(Debug, Clone)]
pub struct Telemetry {
    duration: f64,
    start_utc: Option<DateTime<Utc>>,
    /// Indexed by `Metric::index()`; None when the source has no such data.
    series: Vec<Option<Series>>,
    availability: Availability,
    gps: Vec<GpsPoint>,
    track: Vec<TrackPoint>,
    warnings: Vec<String>,
}

fn shift(t: DateTime<Utc>, seconds: f64) -> DateTime<Utc> {
    t + TimeDelta::microseconds((seconds * 1e6).round() as i64)
}

fn gps_series(gps: &[GpsPoint], interp: Interp, f: impl Fn(&GpsPoint) -> Option<f64>) -> Series {
    Series::new(interp, gps.iter().map(|p| (p.t, p.end, f(p))))
}

fn axis_series(samples: &[Sample<3>], axis: usize, interp: Interp) -> Series {
    Series::new(
        interp,
        samples.iter().map(|s| (s.t, s.end, Some(s.v[axis]))),
    )
}

impl Telemetry {
    pub fn from_gpmf_packets(packets: &[RawPacket]) -> Result<Telemetry, TelemetryError> {
        Self::from_gpmf_packets_with(packets, &TelemetryOptions::default())
    }

    pub fn from_gpmf_packets_with(
        packets: &[RawPacket],
        opts: &TelemetryOptions,
    ) -> Result<Telemetry, TelemetryError> {
        let mut ex = extract(packets);
        if ex.parsed_packets == 0 {
            if let Some(first) = ex.first_error.take() {
                return Err(TelemetryError::Unreadable {
                    packets: packets.len(),
                    first,
                });
            }
            return Ok(Telemetry::empty(ex.duration));
        }
        lock::apply(&mut ex.gps, &opts.lock);
        derive(&mut ex.gps);
        Ok(Self::assemble(ex))
    }

    /// Builds the telemetry from extracted data whose GPS points are already
    /// lock-filtered and derived.
    fn assemble(mut ex: Extracted) -> Telemetry {
        // The original shows the accelerometer through a per-axis Kalman.
        let mut k = [Kalman::default(), Kalman::default(), Kalman::default()];
        for s in &mut ex.accl {
            for (axis, v) in s.v.iter_mut().enumerate() {
                *v = k[axis].update(*v);
            }
        }

        let mut series: Vec<Option<Series>> = vec![None; Metric::COUNT];
        let mut set = |m: Metric, s: Series| series[m.index()] = Some(s);
        let g = &ex.gps;
        if !g.is_empty() {
            use Interp::{Angle180, Angle360, Linear, Step};
            set(Metric::Lat, gps_series(g, Linear, |p| p.derived.lat));
            set(Metric::Lon, gps_series(g, Linear, |p| p.derived.lon));
            set(Metric::Alt, gps_series(g, Linear, |p| p.derived.alt));
            set(Metric::Speed, gps_series(g, Linear, |p| p.derived.speed));
            set(Metric::CSpeed, gps_series(g, Linear, |p| p.derived.cspeed));
            set(Metric::Accel, gps_series(g, Linear, |p| p.derived.accel));
            set(Metric::Dist, gps_series(g, Linear, |p| p.derived.dist));
            set(Metric::COdo, gps_series(g, Linear, |p| p.derived.codo));
            // GoPro has no odometer of its own: the original falls back to codo.
            set(Metric::Odo, gps_series(g, Linear, |p| p.derived.codo));
            set(Metric::CGrad, gps_series(g, Linear, |p| p.derived.cgrad));
            // ... and to cgrad for gradient.
            set(Metric::Gradient, gps_series(g, Linear, |p| p.derived.cgrad));
            set(Metric::Azi, gps_series(g, Angle180, |p| p.derived.azi));
            set(Metric::Cog, gps_series(g, Angle360, |p| p.derived.cog));
            set(Metric::GpsDop, gps_series(g, Step, |p| Some(p.dop)));
            set(
                Metric::GpsLock,
                gps_series(g, Step, |p| Some(f64::from(p.lock.code()))),
            );
        }
        if !ex.accl.is_empty() {
            set(Metric::AcclX, axis_series(&ex.accl, 0, Interp::Linear));
            set(Metric::AcclY, axis_series(&ex.accl, 1, Interp::Linear));
            set(Metric::AcclZ, axis_series(&ex.accl, 2, Interp::Linear));
        }
        if !ex.grav.is_empty() {
            set(Metric::GravX, axis_series(&ex.grav, 0, Interp::Linear));
            set(Metric::GravY, axis_series(&ex.grav, 1, Interp::Linear));
            set(Metric::GravZ, axis_series(&ex.grav, 2, Interp::Linear));
        }
        if !ex.ori.is_empty() {
            set(Metric::OriPitch, axis_series(&ex.ori, 0, Interp::Step));
            set(Metric::OriRoll, axis_series(&ex.ori, 1, Interp::Step));
            set(Metric::OriYaw, axis_series(&ex.ori, 2, Interp::Angle180));
        }
        if !ex.temp.is_empty() {
            let temp = ex.temp.iter().map(|s| (s.t, s.end, Some(s.v[0])));
            set(Metric::Temp, Series::new(Interp::Linear, temp));
        }

        // UTC at file time 0: from the first locked point, else from any
        // GPSU (receivers usually know the time before they get a fix).
        let start_utc = g
            .iter()
            .filter(|p| p.lock.is_locked())
            .chain(g.iter())
            .find_map(|p| p.utc.map(|u| shift(u, -p.t)));
        let track = g
            .iter()
            .filter_map(|p| {
                Some(TrackPoint {
                    t: p.t,
                    lat: p.derived.lat?,
                    lon: p.derived.lon?,
                    alt: p.derived.alt?,
                })
            })
            .collect();
        let availability = availability(&series, ex.duration);
        Telemetry {
            duration: ex.duration,
            start_utc,
            series,
            availability,
            gps: ex.gps,
            track,
            warnings: ex.warnings,
        }
    }

    /// Telemetry of a video without any: every metric is Absent.
    pub fn empty(duration: f64) -> Telemetry {
        let series = vec![None; Metric::COUNT];
        Telemetry {
            duration,
            start_utc: None,
            availability: availability(&series, duration),
            series,
            gps: Vec::new(),
            track: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// End of the last GPMF packet (the video duration when the track
    /// spans the whole video), or the duration given to `empty`.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn start_utc(&self) -> Option<DateTime<Utc>> {
        self.start_utc
    }

    pub fn availability(&self) -> &Availability {
        &self.availability
    }

    pub fn sample(&self, t: f64) -> Snapshot {
        let values: [Value; Metric::COUNT] = std::array::from_fn(|i| {
            self.series[i]
                .as_ref()
                .map_or(Value::Absent, |s| s.sample(t))
        });
        let gps_lock = values[Metric::GpsLock.index()]
            .last_known()
            .map_or(GpsLock::Unknown, |code| GpsLock::from_fix(code as u32));
        Snapshot {
            t,
            utc: self.start_utc.map(|u| shift(u, t)),
            gps_lock,
            values,
            available: std::array::from_fn(|i| self.availability.is_available(Metric::ALL[i])),
        }
    }

    /// Locked GPS points, smoothed as displayed.
    pub fn track(&self) -> &[TrackPoint] {
        &self.track
    }

    /// Every GPS sample, raw and derived (for the dump CLI and tests).
    pub fn gps_points(&self) -> &[GpsPoint] {
        &self.gps
    }

    /// Non-fatal problems met while reading (skipped packets, streams).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

fn availability(series: &[Option<Series>], duration: f64) -> Availability {
    let per_metric = series
        .iter()
        .map(|s| {
            let covered = s.as_ref().map(Series::covered).unwrap_or_default();
            let (gaps, coverage) = gaps_and_coverage(&covered, duration);
            (coverage, gaps)
        })
        .collect();
    Availability { per_metric }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    /// `fixes.len()` one-second packets of 18 GPS points heading north at
    /// about 1 m/s, packet k with GPSF `fixes[k]` (DOP 99.99 when unlocked).
    /// The recorded speed differs from point to point (by more than GPS5's
    /// 1 mm/s resolution): a fix that repeats the previous unlocked speed
    /// would be ignored (see `lock`).
    fn gps_packets(fixes: &[u32]) -> Vec<RawPacket> {
        fixes
            .iter()
            .enumerate()
            .map(|(k, &fix)| {
                let points: Vec<Gps5> = (0..18)
                    .map(|i| {
                        let t = k as f64 + i as f64 / 18.0;
                        let v = 1.0 + 0.01 * i as f64;
                        [45.0 + t / 111_131.745, 7.0, 100.0 + t, v, v]
                    })
                    .collect();
                let dop = if fix >= 2 { 150 } else { 9999 };
                let gpsu = format!("2405011000{k:02}.000");
                RawPacket {
                    pts: k as f64,
                    duration: 1.0,
                    data: devc(&[gps5_stream(&gpsu, fix, dop, &points)]),
                }
            })
            .collect()
    }

    #[test]
    fn telemetry_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Telemetry>();
        assert_send_sync::<Snapshot>();
    }

    #[test]
    fn empty_has_nothing() {
        let tel = Telemetry::empty(10.0);
        assert_eq!(tel.duration(), 10.0);
        let snap = tel.sample(1.0);
        assert_eq!(snap.get(Metric::Speed), Value::Absent);
        assert_eq!(snap.gps_lock, GpsLock::Unknown);
        assert_eq!(snap.utc, None);
        assert_eq!(tel.availability().coverage(Metric::Lat), 0.0);
        assert_eq!(tel.availability().gaps(Metric::Lat), &[(0.0, 10.0)]);
        assert!(
            Telemetry::from_gpmf_packets(&[])
                .unwrap()
                .track()
                .is_empty()
        );
    }

    #[test]
    fn locked_track_is_present_everywhere() {
        let tel = Telemetry::from_gpmf_packets(&gps_packets(&[3; 8])).unwrap();
        assert_eq!(tel.duration(), 8.0);
        assert_eq!(tel.gps_points().len(), 144);
        assert_eq!(tel.track().len(), 144);
        let snap = tel.sample(2.5);
        assert_eq!(snap.gps_lock, GpsLock::Lock3d);
        assert!(snap.get(Metric::Lat).present().is_some());
        let speed = snap.get(Metric::Speed).present().unwrap();
        assert!(speed > 1.0 && speed < 1.17, "{speed}");
        assert!((snap.get(Metric::CSpeed).present().unwrap() - 1.0).abs() < 0.02);
        assert_eq!(snap.get(Metric::GpsDop), Value::Present(1.5));
        assert_eq!(
            snap.utc.unwrap().to_rfc3339(),
            "2024-05-01T10:00:02.500+00:00"
        );
        assert_eq!(
            tel.start_utc().unwrap().to_rfc3339(),
            "2024-05-01T10:00:00+00:00"
        );
        assert_eq!(tel.availability().coverage(Metric::Lat), 1.0);
        assert!(tel.availability().gaps(Metric::Lat).is_empty());
        assert!(!tel.availability().is_available(Metric::AcclX));
        assert!(!tel.availability().is_available(Metric::Hr));
        assert_eq!(snap.get(Metric::Hr), Value::Absent);
    }

    #[test]
    fn lost_fix_is_a_stale_gap() {
        let tel = Telemetry::from_gpmf_packets(&gps_packets(&[3, 3, 0, 0, 3])).unwrap();
        let lat = tel.sample(2.5).get(Metric::Lat);
        let Value::Stale { value, age } = lat else {
            panic!("{lat:?}")
        };
        assert!((age - 0.5).abs() < 1e-9);
        assert_eq!(Some(value), tel.sample(1.99).get(Metric::Lat).present());
        assert_eq!(tel.sample(2.5).gps_lock, GpsLock::NoLock);
        assert!(tel.sample(4.5).get(Metric::Lat).present().is_some());
        assert!((tel.availability().coverage(Metric::Lat) - 0.6).abs() < 1e-9);
        assert_eq!(tel.availability().gaps(Metric::Lat), &[(2.0, 4.0)]);
        // the DOP itself is known throughout
        assert_eq!(tel.availability().coverage(Metric::GpsDop), 1.0);
        assert_eq!(tel.sample(2.5).get(Metric::GpsDop), Value::Present(99.99));
    }

    #[test]
    fn never_locked_gps_is_absent_but_reports_lock_and_dop() {
        let tel = Telemetry::from_gpmf_packets(&gps_packets(&[0, 0, 0])).unwrap();
        for t in [0.0, 1.5, 2.9, 10.0] {
            let snap = tel.sample(t);
            assert_eq!(snap.get(Metric::Lat), Value::Absent, "t={t}");
            assert_eq!(snap.get(Metric::Speed), Value::Absent, "t={t}");
            assert_eq!(snap.gps_lock, GpsLock::NoLock, "t={t}");
        }
        assert_eq!(tel.sample(1.0).get(Metric::GpsLock), Value::Present(0.0));
        assert_eq!(tel.availability().coverage(Metric::Lat), 0.0);
        assert!(tel.track().is_empty());
        // the receiver's clock is still used for the date
        assert_eq!(
            tel.start_utc().unwrap().to_rfc3339(),
            "2024-05-01T10:00:00+00:00"
        );
    }

    #[test]
    fn unreadable_packets() {
        let mut packets = gps_packets(&[3, 3]);
        packets[1].data.truncate(10);
        let tel = Telemetry::from_gpmf_packets(&packets).unwrap();
        assert_eq!(tel.gps_points().len(), 18);
        assert_eq!(tel.warnings().len(), 1);
        assert_eq!(tel.duration(), 2.0);

        packets[0].data.truncate(10);
        let err = Telemetry::from_gpmf_packets(&packets).unwrap_err();
        assert!(
            matches!(err, TelemetryError::Unreadable { packets: 2, .. }),
            "{err}"
        );
    }

    #[test]
    fn accelerometer_and_temperature() {
        let samples = vec![[100i16, 200, 300]; 200];
        let packets: Vec<RawPacket> = (0..2)
            .map(|k| RawPacket {
                pts: f64::from(k),
                duration: 1.0,
                data: devc(&[accl_stream(None, 10, 40.0 + k as f32, &samples)]),
            })
            .collect();
        let tel = Telemetry::from_gpmf_packets(&packets).unwrap();
        let snap = tel.sample(1.5);
        assert_eq!(snap.get(Metric::AcclX), Value::Present(20.0));
        assert_eq!(snap.get(Metric::AcclZ), Value::Present(10.0));
        assert_eq!(snap.get(Metric::Temp), Value::Present(41.0));
        assert_eq!(tel.sample(0.5).get(Metric::Temp), Value::Present(40.5));
        assert_eq!(snap.get(Metric::Lat), Value::Absent);
        assert_eq!(snap.gps_lock, GpsLock::Unknown);
        assert_eq!(tel.availability().coverage(Metric::AcclY), 1.0);
    }

    #[test]
    fn lock_options_are_applied() {
        let opts = TelemetryOptions {
            lock: LockOptions {
                dop_max: 1.0,
                speed_max: None,
            },
        };
        let tel = Telemetry::from_gpmf_packets_with(&gps_packets(&[3, 3]), &opts).unwrap();
        assert_eq!(tel.sample(0.5).gps_lock, GpsLock::NoLock);
    }

    #[test]
    fn snapshot_is_available_follows_availability() {
        let tel = Telemetry::from_gpmf_packets(&gps_packets(&[3; 4])).unwrap();
        let snap = tel.sample(1.0);
        assert!(snap.is_available(Metric::Lat));
        assert!(snap.is_available(Metric::Speed));
        assert!(!snap.is_available(Metric::AcclX));
        assert!(!snap.is_available(Metric::Hr));
        for m in Metric::ALL {
            assert_eq!(snap.is_available(m), tel.availability().is_available(m));
        }
        // also outside the video and for the clone
        assert!(tel.sample(100.0).clone().is_available(Metric::Lat));
        assert!(!Telemetry::empty(5.0).sample(1.0).is_available(Metric::Lat));
    }

    #[test]
    fn angles_cross_north_without_passing_180() {
        use crate::extract::Derived;
        let point = |t: f64, azi: f64, cog: f64| GpsPoint {
            packet: 0,
            index: 0,
            t,
            end: t + 1.0,
            utc: None,
            lat: 45.0,
            lon: 7.0,
            alt: 0.0,
            speed2d: 5.0,
            speed3d: 5.0,
            fix: 3,
            dop: 1.5,
            lock: GpsLock::Lock3d,
            derived: Derived {
                lat: Some(45.0),
                lon: Some(7.0),
                alt: Some(0.0),
                azi: Some(azi),
                cog: Some(cog),
                ..Derived::default()
            },
        };
        let ex = Extracted {
            gps: vec![point(0.0, -10.0, 350.0), point(1.0, 10.0, 10.0)],
            duration: 2.0,
            parsed_packets: 1,
            ..Extracted::default()
        };
        let tel = Telemetry::assemble(ex);
        let mut saw_cross = false;
        for i in 0..=100 {
            let snap = tel.sample(f64::from(i) * 0.01);
            let azi = snap.get(Metric::Azi).present().unwrap();
            let cog = snap.get(Metric::Cog).present().unwrap();
            assert!(azi.abs() <= 10.0 + 1e-9, "azi {azi}");
            assert!(!(10.0 + 1e-9..=350.0 - 1e-9).contains(&cog), "cog {cog}");
            assert!((0.0..360.0).contains(&cog));
            saw_cross |= cog > 355.0 || cog < 5.0;
        }
        assert!(saw_cross);
        // midpoint: azi 0, cog 0 (never 180)
        let mid = tel.sample(0.5);
        assert!(mid.get(Metric::Azi).present().unwrap().abs() < 1e-9);
        assert!(mid.get(Metric::Cog).present().unwrap().abs() < 1e-9);
    }
}
