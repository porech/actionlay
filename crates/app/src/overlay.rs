//! Overlay render thread (spec §3): renders the layout for the time of the frame on
//! screen, at the on-screen size in physical pixels, ~25 Hz while playing and
//! immediately otherwise. Frames are premultiplied RGBA pixmaps handed to the UI and
//! given back for reuse.
//!
//! Concurrency: one render thread, one shared [`State`] behind a mutex and a condition
//! variable. Only the latest request is kept (older ones are dropped, never queued);
//! frames finish in request order, so a frame never replaces a newer one; a frame
//! rendered for a replaced layout or telemetry is discarded. At most one frame waits
//! for the UI, one is being rendered and [`MAX_SPARE`] are kept for reuse.
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use actionlay_layout::Layout;
use actionlay_layout::geom::ScaleMode;
use actionlay_render::tiny_skia::Pixmap;
use actionlay_render::{Renderer, Zone};
use actionlay_telemetry::Telemetry;

/// Render period while playing (25 Hz).
pub const PLAYING_INTERVAL: Duration = Duration::from_millis(40);
/// A forward jump larger than this while playing is a seek, not playback.
const SEEK_JUMP: f64 = 0.25;
/// During a window resize, a new size is rendered at most this often: the renderer
/// drops its glyph and icon caches on every size change.
pub const RESIZE_INTERVAL: Duration = Duration::from_millis(100);
/// Spare pixmaps kept for reuse.
const MAX_SPARE: usize = 2;

/// Physical pixel size of the overlay for a video rect in logical points, clamped
/// (aspect kept) to `max_dim` (the GPU's maximum texture size). `None` below one pixel.
pub fn overlay_size(
    width_points: f32,
    height_points: f32,
    pixels_per_point: f32,
    max_dim: u32,
) -> Option<(u32, u32)> {
    let w = (width_points * pixels_per_point).round();
    let h = (height_points * pixels_per_point).round();
    if !(w >= 1.0 && h >= 1.0) {
        return None; // also rejects NaN
    }
    let k = (max_dim.max(1) as f32 / w.max(h)).min(1.0);
    Some((
        ((w * k).round() as u32).max(1),
        ((h * k).round() as u32).max(1),
    ))
}

/// Everything the overlay image depends on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayKey {
    /// pts of the video frame on screen
    pub t: f64,
    pub width: u32,
    pub height: u32,
    /// Bumped by the UI on every layout or scale mode change.
    pub layout_rev: u64,
    /// Bumped by the UI on every telemetry change (new video, telemetry loaded).
    pub telemetry_rev: u64,
}

impl OverlayKey {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Decides, on the UI thread, when to request an overlay render (spec §3): at once on
/// the first key, a layout/telemetry change, any change while paused and a seek while
/// playing; on a 40 ms grid during playback; and, during a window resize storm, at most
/// every [`RESIZE_INTERVAL`] (the first size change renders at once).
#[derive(Debug, Default)]
pub struct Scheduler {
    /// Last key a render was requested for.
    last: Option<OverlayKey>,
    /// Next slot of the playback grid.
    next_due: Option<Instant>,
    /// When a size change was last requested.
    last_resize: Option<Instant>,
    /// A refused key is still pending: ask again at this time.
    retry_at: Option<Instant>,
}

impl Scheduler {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// When the last refused key becomes due; the UI repaints then (e.g.
    /// `request_repaint_after`) so that the final size of a resize is rendered while
    /// paused. `None` when nothing is pending.
    pub fn retry_at(&self) -> Option<Instant> {
        self.retry_at
    }

    pub fn should_render(&mut self, now: Instant, key: OverlayKey, playing: bool) -> bool {
        let Some(last) = self.last else {
            return self.fire(now, key);
        };
        if last == key {
            self.retry_at = None;
            return false;
        }
        let resized = last.size() != key.size();
        let rebuilt = (last.layout_rev, last.telemetry_rev) != (key.layout_rev, key.telemetry_rev);
        if rebuilt {
            return self.fire(now, key);
        }
        if resized {
            if let Some(until) = self.last_resize.map(|at| at + RESIZE_INTERVAL)
                && now < until
            {
                return self.defer(until);
            }
            return self.fire(now, key);
        }
        let jumped = key.t < last.t || key.t - last.t > SEEK_JUMP;
        if !playing || jumped {
            return self.fire(now, key);
        }
        match self.next_due {
            Some(due) if now < due => self.defer(due),
            Some(due) => {
                // stay on the 40 ms grid unless we fell behind by a whole period
                let late = now.duration_since(due) >= PLAYING_INTERVAL;
                let next = if late { now } else { due } + PLAYING_INTERVAL;
                self.last = Some(key);
                self.next_due = Some(next);
                self.retry_at = None;
                true
            }
            None => self.fire(now, key),
        }
    }

    fn defer(&mut self, until: Instant) -> bool {
        self.retry_at = Some(until);
        false
    }

    fn fire(&mut self, now: Instant, key: OverlayKey) -> bool {
        if self.last.is_some_and(|l| l.size() != key.size()) {
            self.last_resize = Some(now);
        }
        self.last = Some(key);
        self.next_due = Some(now + PLAYING_INTERVAL);
        self.retry_at = None;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayRequest {
    /// pts of the video frame on screen
    pub t: f64,
    pub width: u32,
    pub height: u32,
}

pub struct OverlayFrame {
    /// Premultiplied RGBA; give it back with [`OverlayWorker::recycle`] after upload.
    pub pixmap: Pixmap,
    /// Time it was rendered for.
    pub t: f64,
    pub render_ms: f32,
}

/// One render, with everything read under a single lock so that a layout, telemetry
/// and scale mode change applies to it as a whole.
struct Job {
    request: OverlayRequest,
    pixmap: Pixmap,
    layout: Arc<Layout>,
    telemetry: Arc<Telemetry>,
    scale_mode: ScaleMode,
    generation: u64,
}

struct State {
    layout: Arc<Layout>,
    telemetry: Option<Arc<Telemetry>>,
    scale_mode: ScaleMode,
    /// Bumped on every scene change; frames of an older generation are discarded.
    generation: u64,
    request: Option<OverlayRequest>,
    done: Option<OverlayFrame>,
    spare: Vec<Pixmap>,
    /// Size of the last job: only pixmaps of this size are kept.
    size: Option<(u32, u32)>,
    allocated: usize,
    quit: bool,
}

impl State {
    fn new(layout: Arc<Layout>) -> Self {
        Self {
            layout,
            telemetry: None,
            scale_mode: ScaleMode::Height,
            generation: 0,
            request: None,
            done: None,
            spare: Vec::new(),
            size: None,
            allocated: 0,
            quit: false,
        }
    }

    fn scene_changed(&mut self) {
        self.generation += 1;
        if let Some(old) = self.done.take() {
            self.recycle(old.pixmap);
        }
    }

    fn set_layout(&mut self, layout: Arc<Layout>, scale_mode: ScaleMode) {
        self.layout = layout;
        self.scale_mode = scale_mode;
        self.scene_changed();
    }

    fn set_telemetry(&mut self, telemetry: Option<Arc<Telemetry>>, scale_mode: ScaleMode) {
        self.telemetry = telemetry;
        self.scale_mode = scale_mode;
        self.scene_changed();
    }

    fn recycle(&mut self, pixmap: Pixmap) {
        let current = self
            .size
            .is_none_or(|s| s == (pixmap.width(), pixmap.height()));
        if current && self.spare.len() < MAX_SPARE {
            self.spare.push(pixmap);
        }
    }

    /// Takes the latest request, if telemetry is set, with a pixmap of its size.
    fn next_job(&mut self) -> Option<Job> {
        let telemetry = self.telemetry.clone()?;
        let request = self.request.take()?;
        let size = (request.width, request.height);
        if self.size != Some(size) {
            self.spare.clear(); // other sizes are obsolete
            self.size = Some(size);
        }
        let pixmap = match self.spare.pop() {
            Some(p) => p,
            None => {
                let Some(p) = Pixmap::new(size.0, size.1) else {
                    log::warn!("cannot allocate a {}x{} overlay", size.0, size.1);
                    return None;
                };
                self.allocated += 1;
                p
            }
        };
        Some(Job {
            request,
            pixmap,
            layout: self.layout.clone(),
            telemetry,
            scale_mode: self.scale_mode,
            generation: self.generation,
        })
    }

    /// Publishes a finished frame; false (and the pixmap recycled) when the scene
    /// changed while it rendered. A frame the UI did not take yet is replaced: frames
    /// finish in request order, so the new one is never older.
    fn deliver(&mut self, frame: OverlayFrame, generation: u64) -> bool {
        if generation != self.generation {
            self.recycle(frame.pixmap);
            return false;
        }
        if let Some(old) = self.done.replace(frame) {
            self.recycle(old.pixmap);
        }
        true
    }
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

impl Shared {
    /// A panic on the other thread must not take this one down with it.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Owns the overlay render thread; dropping it stops and joins the thread (after the
/// render in progress, if any).
pub struct OverlayWorker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl OverlayWorker {
    /// `notify` is called on the render thread after each finished frame (the app
    /// passes `egui::Context::request_repaint`). Nothing renders until telemetry is set.
    pub fn spawn(layout: Arc<Layout>, notify: impl Fn() + Send + 'static) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::new(layout)),
            wake: Condvar::new(),
        });
        let worker_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("overlay".into())
            .spawn(move || run(&worker_shared, &notify))
            .expect("spawn overlay thread");
        Self {
            shared,
            thread: Some(thread),
        }
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let r = f(&mut self.shared.lock());
        self.shared.wake.notify_one();
        r
    }

    /// A new layout and the scale mode that goes with it, applied together.
    pub fn set_layout(&self, layout: Arc<Layout>, scale_mode: ScaleMode) {
        self.with_state(|st| st.set_layout(layout, scale_mode));
    }

    /// `None` while a video's telemetry is loading: nothing is rendered. The scale mode
    /// depends on the video, so it changes with it.
    pub fn set_telemetry(&self, telemetry: Option<Arc<Telemetry>>, scale_mode: ScaleMode) {
        self.with_state(|st| st.set_telemetry(telemetry, scale_mode));
    }

    /// Replaces any request not yet started (only the latest matters).
    pub fn request(&self, request: OverlayRequest) {
        self.with_state(|st| st.request = Some(request));
    }

    pub fn take_frame(&self) -> Option<OverlayFrame> {
        self.shared.lock().done.take()
    }

    pub fn recycle(&self, pixmap: Pixmap) {
        self.shared.lock().recycle(pixmap);
    }

    /// Pixmaps created so far (tests check that frames reuse them).
    #[cfg(test)]
    pub fn pixmaps_allocated(&self) -> usize {
        self.shared.lock().allocated
    }
}

impl Drop for OverlayWorker {
    fn drop(&mut self) {
        self.with_state(|st| st.quit = true);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(shared: &Shared, notify: &dyn Fn()) {
    let mut renderer = Renderer::new();
    renderer.set_zone(Zone::System);
    loop {
        let mut job = {
            let mut st = shared.lock();
            loop {
                if st.quit {
                    return;
                }
                if let Some(job) = st.next_job() {
                    break job;
                }
                st = shared.wake.wait(st).unwrap_or_else(PoisonError::into_inner);
            }
        };
        let start = Instant::now();
        renderer.set_scale_mode(job.scale_mode);
        let snapshot = job.telemetry.sample(job.request.t);
        renderer.render_into(&job.layout, &snapshot, &mut job.pixmap);
        let frame = OverlayFrame {
            pixmap: job.pixmap,
            t: job.request.t,
            render_ms: start.elapsed().as_secs_f32() * 1000.0,
        };
        let delivered = {
            let mut st = shared.lock();
            if st.quit {
                return;
            }
            st.deliver(frame, job.generation)
        };
        if delivered {
            notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn key(t: f64) -> OverlayKey {
        OverlayKey {
            t,
            width: 1280,
            height: 960,
            layout_rev: 1,
            telemetry_rev: 1,
        }
    }

    #[test]
    fn overlay_size_uses_physical_pixels_and_clamps() {
        assert_eq!(overlay_size(800.0, 450.0, 2.0, 8192), Some((1600, 900)));
        assert_eq!(overlay_size(100.0, 75.0, 1.5, 8192), Some((150, 113)));
        assert_eq!(overlay_size(6000.0, 3000.0, 2.0, 8192), Some((8192, 4096)));
        assert_eq!(overlay_size(0.3, 0.3, 2.0, 8192), Some((1, 1)));
        assert_eq!(overlay_size(0.0, 100.0, 2.0, 8192), None);
        assert_eq!(overlay_size(0.2, 0.2, 2.0, 8192), None);
        assert_eq!(overlay_size(f32::NAN, 100.0, 2.0, 8192), None);
    }

    #[test]
    fn first_request_renders_and_identical_keys_do_not() {
        let mut s = Scheduler::default();
        let t0 = Instant::now();
        assert!(s.should_render(t0, key(1.0), false));
        assert!(!s.should_render(t0 + Duration::from_millis(500), key(1.0), false));
        assert!(!s.should_render(t0 + Duration::from_millis(500), key(1.0), true));
        assert_eq!(s.retry_at(), None, "nothing pending");
    }

    #[test]
    fn paused_position_change_requests_immediately() {
        let mut s = Scheduler::default();
        let t0 = Instant::now();
        assert!(s.should_render(t0, key(1.0), false));
        // frame step / precise seek while paused: no throttling
        for i in 1..5 {
            let now = t0 + Duration::from_millis(i);
            assert!(s.should_render(now, key(1.0 + i as f64 * 0.01), false));
        }
    }

    #[test]
    fn seek_while_playing_requests_immediately() {
        let mut s = Scheduler::default();
        let t0 = Instant::now();
        assert!(s.should_render(t0, key(5.0), true));
        assert!(
            !s.should_render(t0 + Duration::from_millis(5), key(5.01), true),
            "throttled"
        );
        assert_eq!(s.retry_at(), Some(t0 + PLAYING_INTERVAL));
        assert!(
            s.should_render(t0 + Duration::from_millis(6), key(60.0), true),
            "forward seek"
        );
        assert!(
            s.should_render(t0 + Duration::from_millis(7), key(4.0), true),
            "backward seek"
        );
    }

    #[test]
    fn size_layout_or_telemetry_change_requests_immediately() {
        let mut s = Scheduler::default();
        let t0 = Instant::now();
        assert!(s.should_render(t0, key(1.0), true));
        let ms = |n| t0 + Duration::from_millis(n);
        assert!(s.should_render(
            ms(1),
            OverlayKey {
                width: 640,
                ..key(1.0)
            },
            true
        ));
        assert!(s.should_render(
            ms(2),
            OverlayKey {
                width: 640,
                layout_rev: 2,
                ..key(1.0)
            },
            true
        ));
        assert!(s.should_render(
            ms(3),
            OverlayKey {
                width: 640,
                layout_rev: 2,
                telemetry_rev: 2,
                ..key(1.0)
            },
            true
        ));
    }

    fn sized(t: f64, width: u32) -> OverlayKey {
        OverlayKey { width, ..key(t) }
    }

    #[test]
    fn resize_storm_is_throttled_and_the_final_size_is_rendered() {
        let mut s = Scheduler::default();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        assert!(s.should_render(t0, sized(1.0, 1000), false));
        // leading edge: the first size change renders at once
        assert!(s.should_render(ms(10), sized(1.0, 1001), false));
        // a storm of resize events (one per UI frame) within the window: deferred
        for (i, n) in (26..90).step_by(16).enumerate() {
            assert!(!s.should_render(ms(n), sized(1.0, 1002 + i as u32), false));
            assert_eq!(s.retry_at(), Some(ms(10) + RESIZE_INTERVAL));
        }
        // the storm ended at width 1005; the UI repaints at retry_at and gets it
        let last = sized(1.0, 1005);
        assert!(!s.should_render(ms(100), last, false));
        assert!(s.should_render(ms(10) + RESIZE_INTERVAL, last, false));
        assert_eq!(s.retry_at(), None);
        assert!(!s.should_render(ms(200), last, false));
        // once the size is stable for a window, a resize renders at once again
        assert!(s.should_render(ms(400), sized(1.0, 900), false));
    }

    #[test]
    fn layout_change_during_a_resize_storm_still_renders_at_once() {
        let mut s = Scheduler::default();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        assert!(s.should_render(t0, sized(1.0, 1000), false));
        assert!(s.should_render(ms(1), sized(1.0, 1001), false));
        assert!(!s.should_render(ms(2), sized(1.0, 1002), false));
        let relayout = OverlayKey {
            layout_rev: 2,
            ..sized(1.0, 1002)
        };
        assert!(s.should_render(ms(3), relayout, false));
    }

    #[test]
    fn reset_forgets_everything() {
        let mut s = Scheduler::default();
        let t0 = Instant::now();
        assert!(s.should_render(t0, sized(1.0, 1000), true));
        assert!(s.should_render(t0, sized(1.0, 1001), true));
        assert!(!s.should_render(t0, sized(1.0, 1002), true));
        s.reset();
        assert_eq!(s.retry_at(), None);
        assert!(s.should_render(t0, sized(1.0, 1003), true));
    }

    #[test]
    fn playing_renders_at_25_hz_on_60_and_120_hz_displays() {
        for hz in [60.0, 120.0] {
            let mut s = Scheduler::default();
            let t0 = Instant::now();
            let ticks = (2.0 * hz) as u32; // two seconds of UI frames
            let renders = (0..ticks)
                .filter(|&i| {
                    let elapsed = i as f64 / hz;
                    let now = t0 + Duration::from_secs_f64(elapsed);
                    s.should_render(now, key(elapsed), true)
                })
                .count();
            assert!(
                (49..=51).contains(&renders),
                "{hz} Hz display: {renders} renders in 2 s"
            );
        }
    }

    // --- State (the logic the worker runs under its lock) ---

    fn state() -> State {
        State::new(Arc::new(actionlay_layout::default_layout()))
    }

    fn req(t: f64, width: u32) -> OverlayRequest {
        OverlayRequest {
            t,
            width,
            height: 10,
        }
    }

    fn frame(job: Job) -> OverlayFrame {
        OverlayFrame {
            pixmap: job.pixmap,
            t: job.request.t,
            render_ms: 1.0,
        }
    }

    #[test]
    fn state_waits_for_telemetry_and_takes_only_the_latest_request() {
        let mut st = state();
        st.request = Some(req(1.0, 8));
        assert!(st.next_job().is_none(), "no telemetry yet");
        st.telemetry = Some(Arc::new(Telemetry::empty(10.0)));
        for t in [1.0, 2.0, 3.0] {
            st.request = Some(req(t, 8));
        }
        let job = st.next_job().unwrap();
        assert_eq!(job.request.t, 3.0);
        assert!(st.next_job().is_none(), "older requests were dropped");
    }

    #[test]
    fn state_reuses_pixmaps_of_the_same_size_and_evicts_others() {
        let mut st = state();
        st.telemetry = Some(Arc::new(Telemetry::empty(10.0)));
        st.request = Some(req(1.0, 8));
        let a = st.next_job().unwrap();
        assert_eq!(st.allocated, 1);
        st.recycle(a.pixmap);
        st.request = Some(req(2.0, 8));
        let b = st.next_job().unwrap();
        assert_eq!(st.allocated, 1, "same size: reused");
        st.recycle(b.pixmap);
        st.request = Some(req(3.0, 9));
        let c = st.next_job().unwrap();
        assert_eq!((c.pixmap.width(), st.allocated), (9, 2));
        assert!(st.spare.is_empty(), "spares of another size are dropped");
        st.recycle(Pixmap::new(8, 10).unwrap());
        assert!(
            st.spare.is_empty(),
            "a recycled pixmap of an old size is dropped"
        );
        for _ in 0..10 {
            st.recycle(Pixmap::new(9, 10).unwrap());
        }
        assert_eq!(st.spare.len(), MAX_SPARE, "the pool is bounded");
    }

    #[test]
    fn state_delivers_frames_in_order_and_recycles_untaken_ones() {
        let mut st = state();
        st.telemetry = Some(Arc::new(Telemetry::empty(10.0)));
        st.request = Some(req(1.0, 8));
        let a = st.next_job().unwrap();
        st.request = Some(req(2.0, 8));
        // a newer request is pending: the in-flight frame is still delivered,
        // it is newer than what the UI shows
        let gen_a = a.generation;
        assert!(st.deliver(frame(a), gen_a));
        let b = st.next_job().unwrap();
        let gen_b = b.generation;
        assert!(st.deliver(frame(b), gen_b));
        assert_eq!(st.done.as_ref().unwrap().t, 2.0);
        assert_eq!(
            st.spare.len(),
            1,
            "the frame the UI did not take is recycled"
        );
    }

    #[test]
    fn frames_rendered_for_a_replaced_scene_are_discarded() {
        let mut st = state();
        st.telemetry = Some(Arc::new(Telemetry::empty(10.0)));
        st.request = Some(req(1.0, 8));
        let job = st.next_job().unwrap();
        let generation = job.generation;
        // a new video opens while the frame renders
        st.set_telemetry(Some(Arc::new(Telemetry::empty(20.0))), ScaleMode::Fit);
        assert!(!st.deliver(frame(job), generation));
        assert!(st.done.is_none());
        assert_eq!(st.spare.len(), 1, "its pixmap is recycled");
    }

    #[test]
    fn scene_changes_apply_together_to_the_next_job() {
        let mut st = state();
        st.set_telemetry(Some(Arc::new(Telemetry::empty(10.0))), ScaleMode::Height);
        st.request = Some(req(1.0, 8));
        let first = st.next_job().unwrap();
        let generation = first.generation;
        assert!(st.deliver(frame(first), generation));
        let mut other = actionlay_layout::default_layout();
        other.name = Some("other".into());
        st.set_layout(Arc::new(other), ScaleMode::Fit);
        assert!(st.done.is_none(), "frames of the old layout are dropped");
        st.request = Some(req(2.0, 8));
        let job = st.next_job().unwrap();
        assert_eq!(job.layout.name.as_deref(), Some("other"));
        assert_eq!(job.scale_mode, ScaleMode::Fit);
        assert_eq!(job.telemetry.duration(), 10.0);
    }

    // --- the worker thread ---

    fn worker() -> (OverlayWorker, Arc<AtomicUsize>) {
        let notified = Arc::new(AtomicUsize::new(0));
        let n = notified.clone();
        let w = OverlayWorker::spawn(Arc::new(actionlay_layout::default_layout()), move || {
            n.fetch_add(1, Ordering::SeqCst);
        });
        (w, notified)
    }

    fn with_telemetry(w: &OverlayWorker) {
        w.set_telemetry(Some(Arc::new(Telemetry::empty(10.0))), ScaleMode::Height);
    }

    fn wait_frame(w: &OverlayWorker) -> OverlayFrame {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(f) = w.take_frame() {
                return f;
            }
            assert!(Instant::now() < deadline, "no overlay frame");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn worker_renders_the_requested_size_and_time_once_telemetry_is_set() {
        let (w, notified) = worker();
        w.request(OverlayRequest {
            t: 1.0,
            width: 64,
            height: 36,
        });
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            w.take_frame().is_none(),
            "no telemetry yet: nothing rendered"
        );
        with_telemetry(&w);
        let f = wait_frame(&w);
        assert_eq!((f.pixmap.width(), f.pixmap.height(), f.t), (64, 36, 1.0));
        assert!(notified.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn worker_delivers_latest_request() {
        let (w, _) = worker();
        with_telemetry(&w);
        for t in [1.0, 2.0, 3.0] {
            w.request(OverlayRequest {
                t,
                width: 32,
                height: 18,
            });
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut last = 0.0;
        loop {
            let f = wait_frame(&w);
            assert!([1.0, 2.0, 3.0].contains(&f.t));
            assert!(f.t > last, "frames arrive in request order");
            last = f.t;
            if f.t == 3.0 {
                break;
            }
            assert!(Instant::now() < deadline);
        }
    }

    #[test]
    fn pixmaps_are_recycled_not_reallocated() {
        let (w, _) = worker();
        with_telemetry(&w);
        for i in 0..6 {
            w.request(OverlayRequest {
                t: f64::from(i),
                width: 320,
                height: 180,
            });
            let f = wait_frame(&w);
            w.recycle(f.pixmap);
        }
        assert!(
            w.pixmaps_allocated() <= 2,
            "allocated {}",
            w.pixmaps_allocated()
        );
    }

    #[test]
    fn dropping_the_worker_stops_its_thread_promptly() {
        let alive = Arc::new(());
        let token = alive.clone();
        let w = OverlayWorker::spawn(Arc::new(actionlay_layout::default_layout()), move || {
            let _ = &token;
        });
        with_telemetry(&w);
        // a render is likely in flight when the worker is dropped
        w.request(OverlayRequest {
            t: 1.0,
            width: 640,
            height: 360,
        });
        let start = Instant::now();
        drop(w);
        // the thread owned `notify`: it is gone only once the thread has exited
        assert_eq!(Arc::strong_count(&alive), 1);
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{:?}",
            start.elapsed()
        );

        // idle worker (waiting on its condition variable) too
        let (idle, _) = worker();
        let start = Instant::now();
        drop(idle);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
    }
}
