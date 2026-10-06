//! One metric over time: sampling at any t, and where the data is valid.
use crate::value::Value;

/// Neighbouring valid samples further apart than this are not bridged.
pub(crate) const MAX_BRIDGE: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Interp {
    /// Linear between neighbouring valid samples.
    Linear,
    /// Hold the earlier sample (discrete values).
    Step,
    /// Circular interpolation of an angle in degrees along the shortest arc
    /// (359 -> 1 passes through 0, never 180); the interpolated result is
    /// normalised to [0, 360). For `cog`. Exactly opposite angles (180 apart)
    /// take the negative direction (0 -> 180 passes through 270).
    Angle360,
    /// Same interpolation, result normalised to (-180, 180]. For `azi`,
    /// `lon`, `ori.pitch` and `ori.yaw`. Hold and Stale values are never
    /// altered by either variant.
    Angle180,
}

/// Normalises degrees to [0, 360), never -0.0.
fn norm_deg(a: f64) -> f64 {
    let r = a.rem_euclid(360.0) + 0.0;
    if r >= 360.0 { 0.0 } else { r }
}

/// Normalises degrees to (-180, 180].
fn norm_deg180(a: f64) -> f64 {
    let r = norm_deg(a);
    if r > 180.0 { r - 360.0 } else { r }
}

/// Samples of one metric. Sample k starts at `t[k]`, represents the time
/// until `end[k]` and is valid when `v[k]` is Some.
#[derive(Debug, Clone)]
pub(crate) struct Series {
    fallback: Option<Box<Series>>,
    ranges: Option<Vec<(f64, f64)>>,
    breaks: Vec<f64>,
    t: Vec<f64>,
    end: Vec<f64>,
    v: Vec<Option<f64>>,
    /// Index of the last valid sample at or before k.
    last_valid: Vec<Option<usize>>,
    interp: Interp,
}

impl Series {
    /// `samples` must be sorted by start time.
    pub fn new(interp: Interp, samples: impl IntoIterator<Item = (f64, f64, Option<f64>)>) -> Self {
        let mut s = Series {
            fallback: None,
            ranges: None,
            breaks: Vec::new(),
            t: Vec::new(),
            end: Vec::new(),
            v: Vec::new(),
            last_valid: Vec::new(),
            interp,
        };
        let mut last = None;
        for (t, end, v) in samples {
            debug_assert!(
                s.t.last().is_none_or(|&p| p <= t),
                "series samples must be sorted by start time"
            );
            if v.is_some_and(f64::is_finite) {
                last = Some(s.t.len());
            }
            s.t.push(t);
            s.end.push(end);
            s.v.push(v.filter(|x| x.is_finite()));
            s.last_valid.push(last);
        }
        s
    }

    pub(crate) fn points(&self) -> impl Iterator<Item = (f64, Option<f64>)> + '_ {
        let mut times = self.t.clone();
        if let Some(s) = &self.fallback {
            times.extend(s.points().map(|(t, _)| t));
        }
        times.sort_by(f64::total_cmp);
        times.dedup();
        times.into_iter().map(|t| (t, self.sample(t).present()))
    }

    pub(crate) fn with_fallback(mut self, fallback: Option<Series>) -> Self {
        self.fallback = fallback.map(Box::new);
        self
    }

    pub(crate) fn restrict(&mut self, ranges: Vec<(f64, f64)>) {
        self.ranges = Some(ranges);
    }
    pub(crate) fn with_breaks(mut self, mut breaks: Vec<f64>) -> Self {
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        self.breaks = breaks;
        self
    }

    /// True when valid samples k and k+1 are close enough to be joined.
    fn bridged(&self, k: usize) -> bool {
        k + 1 < self.t.len()
            && self
                .breaks
                .get(self.breaks.partition_point(|&t| t <= self.t[k]))
                .is_none_or(|&t| t > self.t[k + 1])
            && self.v[k].is_some()
            && self.v[k + 1].is_some()
            && self.t[k + 1] - self.t[k] <= MAX_BRIDGE
    }

    pub fn sample(&self, t: f64) -> Value {
        let value = if self
            .ranges
            .as_ref()
            .is_none_or(|rs| rs.iter().any(|&(a, b)| t >= a && t < b))
        {
            self.sample_own(t)
        } else {
            Value::Absent
        };
        if matches!(value, Value::Present(_)) {
            return value;
        }
        if let Some(fallback) = &self.fallback {
            let old = fallback.sample(t);
            if !matches!(old, Value::Absent) {
                return old;
            }
        }
        value
    }

    fn sample_own(&self, t: f64) -> Value {
        // last sample starting at or before t
        let k = self.t.partition_point(|&s| s <= t);
        if k == 0 {
            return Value::Absent;
        }
        let k = k - 1;
        if let Some(v) = self.v[k] {
            if self.bridged(k) && t < self.t[k + 1] {
                let v1 = self.v[k + 1].unwrap_or(v);
                let f = (t - self.t[k]) / (self.t[k + 1] - self.t[k]);
                return Value::Present(match self.interp {
                    Interp::Step => v,
                    Interp::Linear => v + (v1 - v) * f,
                    Interp::Angle360 | Interp::Angle180 => {
                        // shortest signed arc from v to v1, in [-180, 180)
                        let delta = (v1 - v + 540.0).rem_euclid(360.0) - 180.0;
                        let x = v + delta * f;
                        if self.interp == Interp::Angle360 {
                            norm_deg(x)
                        } else {
                            norm_deg180(x)
                        }
                    }
                });
            }
            if t < self.end[k] {
                return Value::Present(v);
            }
        }
        match self.last_valid[k] {
            Some(j) => Value::Stale {
                value: self.v[j].unwrap_or(f64::NAN),
                age: (t - self.end[j]).max(0.0),
            },
            None => Value::Absent,
        }
    }

    /// When the last sample is valid and ends at most `max_gap` before
    /// `until`, holds its value (Present) until `until`. A series whose last
    /// sample is invalid (e.g. lost fix) is left alone: it stays Stale.
    pub fn hold_last_until(&mut self, until: f64, max_gap: f64) {
        if let (Some(Some(_)), Some(end)) = (self.v.last(), self.end.last_mut())
            && *end < until
            && until - *end <= max_gap
        {
            *end = until;
        }
    }

    /// Merged intervals where `sample` is Present.
    pub fn covered(&self) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = Vec::new();
        for k in 0..self.t.len() {
            if self.v[k].is_none() {
                continue;
            }
            let mut end = self.end[k];
            if self.bridged(k) {
                end = end.max(self.t[k + 1]);
            }
            match out.last_mut() {
                Some(last) if self.t[k] <= last.1 => last.1 = last.1.max(end),
                _ => out.push((self.t[k], end)),
            }
        }
        if let Some(ranges) = &self.ranges {
            out = out
                .iter()
                .flat_map(|&(a, b)| {
                    ranges.iter().filter_map(move |&(c, d)| {
                        let span = (a.max(c), b.min(d));
                        (span.1 > span.0).then_some(span)
                    })
                })
                .collect();
        }
        if let Some(s) = &self.fallback {
            out.extend(s.covered());
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for span in out {
            if let Some(last) = merged.last_mut().filter(|p| span.0 <= p.1) {
                last.1 = last.1.max(span.1);
            } else {
                merged.push(span);
            }
        }
        merged
    }
}

/// Gaps of `covered` within `[0, duration]` and the covered fraction.
pub(crate) fn gaps_and_coverage(covered: &[(f64, f64)], duration: f64) -> (Vec<(f64, f64)>, f64) {
    if duration <= 0.0 {
        return (Vec::new(), 0.0);
    }
    let mut gaps = Vec::new();
    let mut cursor = 0.0;
    let mut total = 0.0;
    for &(a, b) in covered {
        let (a, b) = (a.clamp(0.0, duration), b.clamp(0.0, duration));
        if a > cursor {
            gaps.push((cursor, a));
        }
        total += (b - a.max(cursor)).max(0.0);
        cursor = cursor.max(b);
    }
    if cursor < duration {
        gaps.push((cursor, duration));
    }
    (gaps, (total / duration).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(interp: Interp, samples: &[(f64, f64, Option<f64>)]) -> Series {
        Series::new(interp, samples.iter().copied())
    }

    #[test]
    fn interpolates_between_valid_neighbours() {
        let ser = s(
            Interp::Linear,
            &[(0.0, 1.0, Some(0.0)), (1.0, 2.0, Some(10.0))],
        );
        assert_eq!(ser.sample(0.5), Value::Present(5.0));
        assert_eq!(ser.sample(1.5), Value::Present(10.0));
        assert_eq!(
            ser.sample(2.5),
            Value::Stale {
                value: 10.0,
                age: 0.5
            }
        );
        assert_eq!(ser.sample(-0.1), Value::Absent);
        let step = s(
            Interp::Step,
            &[(0.0, 1.0, Some(0.0)), (1.0, 2.0, Some(10.0))],
        );
        assert_eq!(step.sample(0.9), Value::Present(0.0));
    }

    #[test]
    fn hold_last_until_extends_only_a_valid_final_sample() {
        let mut ser = s(
            Interp::Linear,
            &[(0.0, 1.0, Some(0.0)), (1.0, 2.0, Some(10.0))],
        );
        ser.hold_last_until(3.5, MAX_BRIDGE);
        assert_eq!(ser.sample(3.4), Value::Present(10.0));
        assert_eq!(ser.covered(), vec![(0.0, 3.5)]);
        // too far: unchanged
        let mut far = s(Interp::Linear, &[(0.0, 1.0, Some(1.0))]);
        far.hold_last_until(3.5, MAX_BRIDGE);
        assert_eq!(far.covered(), vec![(0.0, 1.0)]);
        // last sample invalid: unchanged
        let mut lost = s(Interp::Linear, &[(0.0, 1.0, Some(1.0)), (1.0, 2.0, None)]);
        lost.hold_last_until(2.5, MAX_BRIDGE);
        assert_eq!(lost.covered(), vec![(0.0, 1.0)]);
        assert!(matches!(lost.sample(2.2), Value::Stale { .. }));
    }

    #[test]
    fn invalid_samples_make_the_last_value_stale() {
        let ser = s(
            Interp::Linear,
            &[
                (0.0, 1.0, Some(1.0)),
                (1.0, 2.0, None),
                (2.0, 3.0, Some(3.0)),
            ],
        );
        assert_eq!(ser.sample(0.5), Value::Present(1.0));
        assert_eq!(
            ser.sample(1.25),
            Value::Stale {
                value: 1.0,
                age: 0.25
            }
        );
        assert_eq!(ser.sample(2.5), Value::Present(3.0));
        assert_eq!(ser.covered(), vec![(0.0, 1.0), (2.0, 3.0)]);
    }

    #[test]
    fn leading_invalid_samples_are_absent() {
        let ser = s(Interp::Linear, &[(0.0, 1.0, None), (1.0, 2.0, Some(5.0))]);
        assert_eq!(ser.sample(0.5), Value::Absent);
        assert_eq!(ser.sample(1.5), Value::Present(5.0));
        assert_eq!(ser.covered(), vec![(1.0, 2.0)]);
        assert!(s(Interp::Linear, &[(0.0, 1.0, None)]).covered().is_empty());
        assert_eq!(s(Interp::Linear, &[]).sample(1.0), Value::Absent);
    }

    #[test]
    fn long_holes_are_not_bridged() {
        let ser = s(
            Interp::Linear,
            &[(0.0, 0.1, Some(0.0)), (5.0, 5.1, Some(10.0))],
        );
        assert_eq!(
            ser.sample(1.0),
            Value::Stale {
                value: 0.0,
                age: 0.9
            }
        );
        assert_eq!(ser.covered(), vec![(0.0, 0.1), (5.0, 5.1)]);
        // a missing packet (1 s) is bridged
        let ser = s(
            Interp::Linear,
            &[(0.0, 0.1, Some(0.0)), (1.0, 1.1, Some(10.0))],
        );
        assert_eq!(ser.sample(0.5), Value::Present(5.0));
        assert_eq!(ser.covered(), vec![(0.0, 1.1)]);
    }

    #[test]
    fn non_finite_values_are_invalid() {
        let ser = s(
            Interp::Linear,
            &[(0.0, 1.0, Some(f64::NAN)), (1.0, 2.0, Some(1.0))],
        );
        assert_eq!(ser.sample(0.5), Value::Absent);
    }

    #[test]
    fn coverage_and_gaps() {
        let (gaps, cov) = gaps_and_coverage(&[(0.0, 1.0), (2.0, 3.0)], 4.0);
        assert_eq!(gaps, vec![(1.0, 2.0), (3.0, 4.0)]);
        assert!((cov - 0.5).abs() < 1e-12);
        let (gaps, cov) = gaps_and_coverage(&[], 4.0);
        assert_eq!((gaps, cov), (vec![(0.0, 4.0)], 0.0));
        let (gaps, cov) = gaps_and_coverage(&[(-1.0, 9.0)], 4.0);
        assert_eq!((gaps, cov), (vec![], 1.0));
        assert_eq!(gaps_and_coverage(&[(0.0, 1.0)], 0.0), (vec![], 0.0));
    }

    fn angle(interp: Interp, a: f64, b: f64, t: f64) -> f64 {
        match s(interp, &[(0.0, 0.1, Some(a)), (1.0, 1.1, Some(b))]).sample(t) {
            Value::Present(x) => x,
            other => panic!("not present: {other:?}"),
        }
    }

    #[test]
    fn angle360_crosses_north_through_zero() {
        let m = angle(Interp::Angle360, 359.0, 1.0, 0.5);
        assert!(m == 0.0 && m.is_sign_positive(), "{m}");
        let m = angle(Interp::Angle360, 10.0, 350.0, 0.5);
        assert!(m == 0.0 && m.is_sign_positive(), "{m}");
        assert!((angle(Interp::Angle360, 359.0, 1.0, 0.25) - 359.5).abs() < 1e-9);
        assert!((angle(Interp::Angle360, 90.0, 100.0, 0.5) - 95.0).abs() < 1e-9);
        assert!((angle(Interp::Angle360, 100.0, 90.0, 0.5) - 95.0).abs() < 1e-9);
    }

    #[test]
    fn angle180_is_signed() {
        assert!((angle(Interp::Angle180, -10.0, -20.0, 0.5) + 15.0).abs() < 1e-9);
        assert!((angle(Interp::Angle180, 90.0, 100.0, 0.5) - 95.0).abs() < 1e-9);
        // 170 -> -170 crosses +-180
        for i in 0..=100 {
            let v = angle(Interp::Angle180, 170.0, -170.0, i as f64 / 100.0);
            assert!(v > -180.0 && v <= 180.0, "{v}");
        }
        assert!((angle(Interp::Angle180, 170.0, -170.0, 0.25) - 175.0).abs() < 1e-9);
        // the midpoint is exactly +-180 and maps to 180
        assert_eq!(angle(Interp::Angle180, 170.0, -170.0, 0.5), 180.0);
        assert_eq!(angle(Interp::Angle180, -170.0, 170.0, 0.5), 180.0);
        let m = angle(Interp::Angle180, 10.0, -10.0, 0.5);
        assert!(m == 0.0 && m.is_sign_positive(), "{m}");
    }

    #[test]
    fn angle_hold_and_stale_keep_the_stored_value() {
        for interp in [Interp::Angle180, Interp::Angle360] {
            let ser = s(interp, &[(0.0, 1.0, Some(-90.0))]);
            assert_eq!(ser.sample(0.5), Value::Present(-90.0));
            assert_eq!(
                ser.sample(1.5),
                Value::Stale {
                    value: -90.0,
                    age: 0.5
                }
            );
        }
    }

    #[test]
    fn angle_tie_goes_the_negative_way() {
        // 0 and 180 are equally far both ways; the code takes delta = -180
        assert_eq!(angle(Interp::Angle360, 0.0, 180.0, 0.5), 270.0);
        assert_eq!(angle(Interp::Angle180, 0.0, 180.0, 0.5), -90.0);
        assert_eq!(angle(Interp::Angle360, 180.0, 0.0, 0.5), 90.0);
    }

    #[test]
    fn boundaries() {
        // gap of exactly MAX_BRIDGE is bridged
        let ser = s(
            Interp::Linear,
            &[(0.0, 0.1, Some(0.0)), (2.0, 2.1, Some(10.0))],
        );
        assert_eq!(ser.sample(1.0), Value::Present(5.0));
        assert_eq!(ser.covered(), vec![(0.0, 2.1)]);
        // t == t[k+1] returns the next sample's value
        assert_eq!(ser.sample(2.0), Value::Present(10.0));
        // t == end[k] with an invalid next sample: Stale with age 0
        let ser = s(Interp::Linear, &[(0.0, 1.0, Some(4.0)), (1.0, 2.0, None)]);
        assert_eq!(ser.sample(0.999), Value::Present(4.0));
        assert_eq!(
            ser.sample(1.0),
            Value::Stale {
                value: 4.0,
                age: 0.0
            }
        );
    }
}
