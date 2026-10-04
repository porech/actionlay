//! The two filters gopro-dashboard-overlay applies, ported for parity
//! (`gopro_overlay/smoothing.py`).

/// Scalar Kalman filter with R = 100, Q = 10, H = 1, P₀ = 0. The first
/// update seeds the estimate, so a constant input passes through unchanged.
#[derive(Debug, Clone, Default)]
pub(crate) struct Kalman {
    p: f64,
    estimate: Option<f64>,
}

impl Kalman {
    const R: f64 = 100.0;
    const Q: f64 = 10.0;

    pub fn update(&mut self, u: f64) -> f64 {
        let est = *self.estimate.get_or_insert(u);
        let k = self.p / (self.p + Self::R);
        let est = est + k * (u - est);
        self.p = (1.0 - k) * self.p + Self::Q;
        self.estimate = Some(est);
        est
    }
}

/// gopro-dashboard-overlay's "simple exponential" smoothing of positions.
/// Its output at step n is `α·x[n−1] + (1−α)·out[n−1]` (it lags one sample),
/// and the first two outputs both equal the first input.
#[derive(Debug, Clone)]
pub(crate) struct Ses {
    alpha: f64,
    previous: Option<[f64; 2]>,
    forecast: Option<[f64; 2]>,
}

impl Ses {
    pub fn new(alpha: f64) -> Self {
        Ses {
            alpha,
            previous: None,
            forecast: None,
        }
    }

    pub fn update(&mut self, x: [f64; 2]) -> [f64; 2] {
        let out = match (self.forecast, self.previous) {
            (Some(f), Some(p)) => {
                let a = self.alpha;
                [a * p[0] + (1.0 - a) * f[0], a * p[1] + (1.0 - a) * f[1]]
            }
            _ => x,
        };
        self.forecast = Some(out);
        self.previous = Some(x);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kalman_matches_the_original() {
        // Values printed by gopro_overlay.smoothing.Kalman for 1, 2, 3, 3.
        let mut k = Kalman::default();
        let out: Vec<f64> = [1.0, 2.0, 3.0, 3.0].iter().map(|&u| k.update(u)).collect();
        assert_eq!(
            out,
            vec![
                1.0,
                1.090_909_090_909_090_8,
                1.396_946_564_885_496_2,
                1.728_043_609_933_373_8
            ]
        );
        let mut k = Kalman::default();
        assert!((0..100).all(|_| k.update(4.2) == 4.2));
    }

    #[test]
    fn ses_lags_one_sample() {
        let mut s = Ses::new(0.45);
        assert_eq!(s.update([0.0, 0.0]), [0.0, 0.0]);
        assert_eq!(s.update([1.0, 10.0]), [0.0, 0.0]);
        assert_eq!(s.update([2.0, 20.0]), [0.45, 4.5]);
        // 0.45·2 + 0.55·0.45
        let out = s.update([3.0, 30.0]);
        assert!((out[0] - 1.1475).abs() < 1e-12);
    }
}
