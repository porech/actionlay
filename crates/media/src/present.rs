//! Choosing which decoded frame to show at a given clock time.
use std::collections::VecDeque;

use crate::frame::Nv12Frame;

/// Returns the most recent frame due at `clock` (if any) and how many older
/// due frames were skipped. Frames in the future stay queued.
pub fn select_frame(queue: &mut VecDeque<Nv12Frame>, clock: f64) -> (Option<Nv12Frame>, usize) {
    let mut chosen = None;
    let mut dropped = 0;
    while let Some(frame) = queue.pop_front_if(|f| f.pts <= clock) {
        if chosen.replace(frame).is_some() {
            dropped += 1;
        }
    }
    (chosen, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(pts: f64) -> Nv12Frame {
        Nv12Frame {
            width: 2,
            height: 2,
            y: vec![0; 4],
            uv: vec![0; 2],
            pts,
        }
    }

    #[test]
    fn nothing_due_yet() {
        let mut q: VecDeque<_> = [f(1.0), f(1.01)].into();
        let (frame, dropped) = select_frame(&mut q, 0.5);
        assert!(frame.is_none());
        assert_eq!((dropped, q.len()), (0, 2));
    }

    #[test]
    fn picks_latest_due_and_drops_older() {
        let mut q: VecDeque<_> = [f(0.00), f(0.01), f(0.02), f(0.03)].into();
        let (frame, dropped) = select_frame(&mut q, 0.025);
        assert_eq!(frame.unwrap().pts, 0.02);
        assert_eq!(dropped, 2);
        assert_eq!(q.len(), 1);
    }
}
