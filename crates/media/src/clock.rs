//! Playback clocks. Audio drives playback at 1x; otherwise a system clock
//! scaled by the playback speed does.
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct SystemClock {
    anchor_media: f64,
    anchor_instant: Instant,
    speed: f64,
    paused: bool,
}

impl SystemClock {
    /// A paused clock positioned at `start` seconds of media time.
    pub fn new(start: f64, now: Instant) -> Self {
        Self {
            anchor_media: start,
            anchor_instant: now,
            speed: 1.0,
            paused: true,
        }
    }

    pub fn time(&self, now: Instant) -> f64 {
        if self.paused {
            self.anchor_media
        } else {
            let elapsed = now
                .saturating_duration_since(self.anchor_instant)
                .as_secs_f64();
            self.anchor_media + elapsed * self.speed
        }
    }

    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        self.rebase(now);
        self.paused = paused;
    }

    pub fn set_speed(&mut self, speed: f64, now: Instant) {
        assert!(speed > 0.0, "speed must be positive");
        self.rebase(now);
        self.speed = speed;
    }

    pub fn seek(&mut self, to: f64, now: Instant) {
        self.anchor_media = to;
        self.anchor_instant = now;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    fn rebase(&mut self, now: Instant) {
        self.anchor_media = self.time(now);
        self.anchor_instant = now;
    }
}

/// Media time of the sample currently leaving the speakers.
pub fn audio_clock_time(
    base_pts: f64,
    frames_played: u64,
    sample_rate: u32,
    output_latency: Duration,
) -> f64 {
    let played = frames_played as f64 / sample_rate as f64;
    (base_pts + played - output_latency.as_secs_f64()).max(base_pts)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn paused_clock_does_not_advance() {
        let t0 = Instant::now();
        let c = SystemClock::new(5.0, t0);
        assert!(c.is_paused());
        assert!(approx(c.time(t0 + Duration::from_secs(3)), 5.0));
    }

    #[test]
    fn running_clock_advances_with_speed() {
        let t0 = Instant::now();
        let mut c = SystemClock::new(0.0, t0);
        c.set_paused(false, t0);
        assert!(approx(c.time(t0 + Duration::from_secs(2)), 2.0));
        c.set_speed(2.0, t0 + Duration::from_secs(2));
        assert!(approx(c.time(t0 + Duration::from_secs(3)), 4.0));
    }

    #[test]
    fn pause_keeps_position_and_seek_moves_it() {
        let t0 = Instant::now();
        let mut c = SystemClock::new(0.0, t0);
        c.set_paused(false, t0);
        c.set_paused(true, t0 + Duration::from_millis(1500));
        assert!(approx(c.time(t0 + Duration::from_secs(10)), 1.5));
        c.seek(42.0, t0 + Duration::from_secs(10));
        assert!(approx(c.time(t0 + Duration::from_secs(11)), 42.0));
    }

    #[test]
    fn audio_clock_subtracts_latency_but_not_below_base() {
        let t = audio_clock_time(10.0, 48_000, 48_000, Duration::from_millis(20));
        assert!(approx(t, 10.98));
        assert!(approx(
            audio_clock_time(10.0, 0, 48_000, Duration::from_millis(20)),
            10.0
        ));
    }
}
