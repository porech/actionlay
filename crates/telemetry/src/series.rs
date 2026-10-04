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
    /// (359 -> 1 passes through 0, never 180). Results are normalised to
    /// [0, 360). MUST be used for `azi`, `cog` and `ori.yaw` when the series
    /// are built (Task 9).
    Angle,
}

/// Normalises degrees to [0, 360).
fn norm_deg(a: f64) -> f64 {
    let r = a.rem_euclid(360.0);
    if r >= 360.0 { 0.0 } else { r }
}

/// Samples of one metric. Sample k starts at `t[k]`, represents the time
/// until `end[k]` and is valid when `v[k]` is Some.
#[derive(Debug, Clone)]
pub(crate) struct Series {
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
            t: Vec::new(),
            end: Vec::new(),
            v: Vec::new(),
            last_valid: Vec::new(),
            interp,
        };
        let mut last = None;
        for (t, end, v) in samples {
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

    /// True when valid samples k and k+1 are close enough to be joined.
    fn bridged(&self, k: usize) -> bool {
        k + 1 < self.t.len()
            && self.v[k].is_some()
            && self.v[k + 1].is_some()
            && self.t[k + 1] - self.t[k] <= MAX_BRIDGE
    }

    /// Output normalisation: angles live in [0, 360).
    fn out(&self, v: f64) -> f64 {
        match self.interp {
            Interp::Angle => norm_deg(v),
            _ => v,
        }
    }

    pub fn sample(&self, t: f64) -> Value {
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
                    Interp::Angle => {
                        // shortest signed arc from v to v1, in [-180, 180)
                        let delta = (v1 - v + 540.0).rem_euclid(360.0) - 180.0;
                        norm_deg(v + delta * f)
                    }
                });
            }
            if t < self.end[k] {
                return Value::Present(self.out(v));
            }
        }
        match self.last_valid[k] {
            Some(j) => Value::Stale {
                value: self.out(self.v[j].unwrap_or(f64::NAN)),
                age: (t - self.end[j]).max(0.0),
            },
            None => Value::Absent,
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
        out
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

    fn angle(a: f64, b: f64, t: f64) -> Value {
        s(Interp::Angle, &[(0.0, 0.1, Some(a)), (1.0, 1.1, Some(b))]).sample(t)
    }

    fn present(v: Value) -> f64 {
        match v {
            Value::Present(x) => x,
            other => panic!("not present: {other:?}"),
        }
    }

    #[test]
    fn angle_crosses_north_through_zero() {
        let m = present(angle(359.0, 1.0, 0.5));
        assert!(m.abs() < 1e-9 && m >= 0.0, "{m}");
        let m = present(angle(10.0, 350.0, 0.5));
        assert!(m.abs() < 1e-9 && m >= 0.0, "{m}");
        // quarter way 359 -> 1 is 359.5, not near 180
        assert!((present(angle(359.0, 1.0, 0.25)) - 359.5).abs() < 1e-9);
        assert!((present(angle(10.0, 350.0, 0.25)) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn angle_without_wrap_is_linear() {
        assert!((present(angle(90.0, 100.0, 0.5)) - 95.0).abs() < 1e-9);
        assert!((present(angle(100.0, 90.0, 0.5)) - 95.0).abs() < 1e-9);
    }

    #[test]
    fn angle_results_are_normalised() {
        // negative inputs (e.g. yaw in (-180, 180]) come out in [0, 360)
        assert!((present(angle(-10.0, -20.0, 0.5)) - 345.0).abs() < 1e-9);
        for i in 0..=100 {
            let v = present(angle(-170.0, 170.0, i as f64 / 100.0));
            assert!((0.0..360.0).contains(&v), "{v}");
        }
        // hold and stale values are normalised too
        let ser = s(Interp::Angle, &[(0.0, 1.0, Some(-90.0))]);
        assert_eq!(ser.sample(0.5), Value::Present(270.0));
        assert_eq!(
            ser.sample(1.5),
            Value::Stale {
                value: 270.0,
                age: 0.5
            }
        );
    }
}
