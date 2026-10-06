//! Playback clocks. Audio drives playback at 1x; otherwise a system clock
//! scaled by the playback speed does.
use std::cell::Cell;
use std::time::{Duration, Instant};

/// Output latency is an estimate and may jump after an underrun. Such changes
/// must not move playback backwards, or move a frozen/empty output at all.
#[derive(Debug)]
pub(crate) struct AudioClock {
    base: f64,
    last: Cell<(u64, f64)>,
}

impl AudioClock {
    pub fn new(base: f64) -> Self {
        Self {
            base,
            last: Cell::new((0, base)),
        }
    }
    pub fn sample(&self, frames: u64, rate: u32, latency: Duration, paused: bool) -> f64 {
        let (previous_frames, previous_time) = self.last.get();
        if paused || frames == previous_frames {
            return previous_time;
        }
        let time = audio_clock_time(self.base, frames, rate, latency).max(previous_time);
        self.last.set((frames, time));
        time
    }
}

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

    #[test]
    fn audio_latency_jitter_and_buffering_do_not_reverse_or_move_a_frozen_clock() {
        let clock = AudioClock::new(10.0);
        assert!(approx(
            clock.sample(48_000, 48_000, Duration::from_millis(20), false),
            10.98
        ));
        // No new audio: changing the driver's latency estimate cannot move us.
        for latency in [0, 200, 10, 100] {
            assert!(approx(
                clock.sample(48_000, 48_000, Duration::from_millis(latency), false),
                10.98
            ));
            assert!(approx(
                clock.sample(50_000, 48_000, Duration::from_millis(latency), true),
                10.98
            ));
        }
        // More samples but a larger reported latency: hold until it catches up.
        assert!(approx(
            clock.sample(48_480, 48_000, Duration::from_millis(100), false),
            10.98
        ));
        assert!(approx(
            clock.sample(52_800, 48_000, Duration::from_millis(20), false),
            11.08
        ));
        // Explicit seek/reset starts an independent timeline and can go back.
        let clock = AudioClock::new(2.0);
        assert!(approx(clock.sample(0, 48_000, Duration::ZERO, false), 2.0));
        assert!(approx(
            clock.sample(480, 48_000, Duration::ZERO, false),
            2.01
        ));
    }
}
