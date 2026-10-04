//! Playback engine: one decode thread feeding a small frame queue and the audio output.
//!
//! Threads and channels:
//! - the UI thread owns [`Player`] and calls [`Player::poll_frame`] once per
//!   UI frame; it keeps at most [`QUEUE_CAP`] decoded frames;
//! - one decode thread ([`Worker`]) owns the demuxer and both decoders, sends
//!   [`Msg`]s on a bounded channel and pushes samples into the shared
//!   [`AudioOutput`]; it receives [`Command`]s on an unbounded channel.
//!
//! Every seek bumps a generation number. Frames and the end-of-stream marker
//! carry the generation they belong to and the player ignores any message of
//! an older generation, so nothing decoded before a seek is ever presented.
//! The worker never blocks for more than [`POLL`] without looking at its
//! commands, so a seek or quit is always picked up promptly.
//!
//! Clock: at 1x while playing with an open audio output the audio clock drives
//! presentation; otherwise (paused, other speeds, no audio, audio missing) the
//! [`SystemClock`] does. A muted output does not consume its buffer, so the
//! audio queued before a pause is stale by the time playback resumes: every
//! transition into audio-driven playback re-seeks precisely to the current
//! position, which flushes the decoders and restarts the audio output.
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{
    Receiver, RecvTimeoutError, Sender, TryRecvError, TrySendError, bounded, unbounded,
};
use ffmpeg_next as ffmpeg;

use crate::{
    MediaError,
    audio::{AudioChunk, AudioDecoder, AudioOutput},
    clock::SystemClock,
    frame::Nv12Frame,
    present::select_frame,
    probe::{MediaInfo, probe},
    video::VideoDecoder,
};

/// Decoded frames in flight on the channel.
const CHANNEL_CAP: usize = 8;
/// Decoded frames held by the player, waiting for their presentation time.
const QUEUE_CAP: usize = 4;
/// Compressed video packets the worker may read ahead to reach audio packets.
const MAX_VIDEO_PACKETS: usize = 512;
/// Longest the worker waits without checking its commands.
const POLL: Duration = Duration::from_millis(5);
/// How long the audio may starve a full video queue, or stand still with
/// samples queued (stalled device), before playback falls back to the system
/// clock.
const AUDIO_STARVATION: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy)]
pub struct PlayerOptions {
    pub prefer_hw: bool,
    pub audio: bool,
}

impl Default for PlayerOptions {
    fn default() -> Self {
        Self {
            prefer_hw: true,
            audio: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlayerStats {
    pub backend: &'static str,
    pub dropped: u64,
    pub presented: u64,
    /// An audio output is open for this file.
    pub audio_active: bool,
    /// Last presented frame minus the audio clock (0 when audio is not driving).
    pub av_offset: f64,
}

enum Command {
    Seek {
        to: f64,
        precise: bool,
        generation: u64,
    },
    Quit,
}

enum Msg {
    Frame {
        generation: u64,
        frame: Nv12Frame,
        /// When the decode thread produced it.
        ready: Instant,
    },
    /// No more frames for this generation (end of file).
    End { generation: u64 },
}

/// State shared between the player and the decode thread.
struct Shared {
    /// Latest generation requested by the player.
    generation: AtomicU64,
    /// The player wants audio for the current generation (audio is driving).
    audio_wanted: AtomicBool,
    backend: Mutex<&'static str>,
}

pub struct Player {
    info: MediaInfo,
    commands: Sender<Command>,
    msgs: Receiver<Msg>,
    queue: VecDeque<Nv12Frame>,
    shared: Arc<Shared>,
    generation: u64,
    audio: Option<Arc<Mutex<AudioOutput>>>,
    /// Audio stopped arriving for this generation; the system clock drives.
    audio_lost: bool,
    /// The output device stopped consuming samples. Sticky for the lifetime
    /// of the player (a seek would only freeze again on a dead device): the
    /// system clock drives from then on; reopening the file retries audio.
    audio_dead: bool,
    stall: StallDetector,
    starving_since: Option<Instant>,
    clock: SystemClock,
    position: f64,
    awaiting_seek_frame: bool,
    /// When the first frame after a seek became available.
    seek_frame_ready: Option<Instant>,
    /// Last time playback was resumed; paused time never counts.
    resumed_at: Instant,
    end_seen: bool,
    dropped: u64,
    presented: u64,
    last_frame_pts: f64,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    pub fn open(path: &Path, options: PlayerOptions) -> Result<Self, MediaError> {
        let info = probe(path)?;
        let audio = if options.audio && info.audio.is_some() {
            match AudioOutput::open() {
                Ok(out) => {
                    out.set_muted(true); // starts paused
                    Some(Arc::new(Mutex::new(out)))
                }
                Err(e) => {
                    log::warn!("audio disabled: {e}");
                    None
                }
            }
        } else {
            None
        };

        let (cmd_tx, cmd_rx) = unbounded();
        let (msg_tx, msg_rx) = bounded(CHANNEL_CAP);
        let shared = Arc::new(Shared {
            generation: AtomicU64::new(0),
            audio_wanted: AtomicBool::new(false),
            backend: Mutex::new("unknown"),
        });

        let worker = Worker {
            path: path.to_path_buf(),
            info: info.clone(),
            prefer_hw: options.prefer_hw,
            audio: audio.clone(),
            commands: cmd_rx,
            msgs: msg_tx,
            shared: shared.clone(),
        };
        let thread = std::thread::Builder::new()
            .name("actionlay-decode".into())
            .spawn(move || {
                if let Err(e) = worker.run() {
                    log::error!("decode thread stopped: {e}");
                }
            })
            .map_err(|e| MediaError::Audio(format!("cannot spawn the decode thread: {e}")))?;

        Ok(Self {
            info,
            commands: cmd_tx,
            msgs: msg_rx,
            queue: VecDeque::new(),
            shared,
            generation: 0,
            audio,
            audio_lost: false,
            audio_dead: false,
            stall: StallDetector::default(),
            starving_since: None,
            clock: SystemClock::new(0.0, Instant::now()),
            position: 0.0,
            awaiting_seek_frame: true,
            seek_frame_ready: None,
            resumed_at: Instant::now(),
            end_seen: false,
            dropped: 0,
            presented: 0,
            last_frame_pts: 0.0,
            thread: Some(thread),
        })
    }

    pub fn info(&self) -> &MediaInfo {
        &self.info
    }

    pub fn is_paused(&self) -> bool {
        self.clock.is_paused()
    }

    pub fn play(&mut self) {
        if !self.is_paused() {
            return;
        }
        let restart = self.at_end();
        if restart {
            self.position = 0.0;
        }
        let now = Instant::now();
        self.resumed_at = now;
        self.clock.seek(self.position, now);
        self.clock.set_paused(false, now);
        if restart || self.audio_can_drive() {
            // Audio queued before the pause is stale: start it again from here.
            self.seek(self.position, true);
        } else {
            self.update_audio_mode();
        }
    }

    pub fn pause(&mut self) {
        if self.is_paused() {
            return;
        }
        self.position = self.current_time();
        let now = Instant::now();
        self.clock.seek(self.position, now);
        self.clock.set_paused(true, now);
        self.update_audio_mode();
    }

    pub fn toggle(&mut self) {
        if self.is_paused() {
            self.play()
        } else {
            self.pause()
        }
    }

    pub fn speed(&self) -> f64 {
        self.clock.speed()
    }

    pub fn set_speed(&mut self, speed: f64) {
        if !(speed.is_finite() && speed > 0.0) {
            log::warn!("ignoring invalid playback speed {speed}");
            return;
        }
        let was_driven = self.audio_driven();
        self.position = self.current_time();
        let now = Instant::now();
        self.clock.seek(self.position, now);
        self.clock.set_speed(speed, now);
        if !was_driven && !self.is_paused() && self.audio_can_drive() {
            // Back to audio-driven playback: the buffered audio is stale.
            self.seek(self.position, true);
        } else {
            self.update_audio_mode();
        }
    }

    pub fn seek(&mut self, to: f64, precise: bool) {
        // Unknown duration (0): only clamp at the start.
        let last = if self.info.duration > 0.0 {
            (self.info.duration - self.frame_duration()).max(0.0)
        } else {
            f64::INFINITY
        };
        let to = if to.is_nan() {
            0.0
        } else {
            to.clamp(0.0, last)
        };
        self.generation += 1;
        let generation = self.generation;
        // Publish the generation before touching the audio output: the worker
        // checks it under the same lock before every push, so no audio of an
        // older generation can land in the buffer after this reset.
        self.shared.generation.store(generation, Ordering::SeqCst);
        if let Some(a) = &self.audio {
            a.lock().unwrap().reset(to);
        }
        self.queue.clear();
        self.end_seen = false;
        self.audio_lost = false;
        self.stall.reset();
        self.starving_since = None;
        self.awaiting_seek_frame = true;
        self.seek_frame_ready = None;
        self.position = to;
        self.clock.seek(to, Instant::now());
        // Decide whether the worker should feed audio before it sees the seek.
        self.update_audio_mode();
        let _ = self.commands.send(Command::Seek {
            to,
            precise,
            generation,
        });
    }

    pub fn step(&mut self, frames: i32) {
        self.pause();
        let from = if self.awaiting_seek_frame {
            self.position
        } else {
            self.last_frame_pts
        };
        self.seek(from + f64::from(frames) * self.frame_duration(), true);
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    pub fn at_end(&self) -> bool {
        self.end_seen && self.queue.is_empty() && !self.awaiting_seek_frame
    }

    pub fn stats(&self) -> PlayerStats {
        let av_offset = if self.audio_driven() {
            self.last_frame_pts - self.current_time()
        } else {
            0.0
        };
        PlayerStats {
            backend: *self.shared.backend.lock().unwrap(),
            dropped: self.dropped,
            presented: self.presented,
            audio_active: self.audio.is_some() && !self.audio_dead,
            av_offset,
        }
    }

    /// Call once per UI frame: returns the frame to show now, if it changed.
    pub fn poll_frame(&mut self) -> Option<Nv12Frame> {
        self.receive();
        if self.awaiting_seek_frame {
            let f = self.queue.pop_front()?;
            self.awaiting_seek_frame = false;
            self.position = f.pts;
            // The clock restarts from the first frame, counted from when the
            // frame was ready rather than from when the UI got to poll it.
            let ready = self.seek_frame_ready.take().unwrap_or_else(Instant::now);
            self.clock
                .seek(f.pts, ready.max(self.resumed_at).min(Instant::now()));
            return Some(self.present(f));
        }
        if self.is_paused() {
            return None;
        }
        self.watch_audio();
        let now = self.current_time();
        if self.audio_driven() {
            // Keep the system clock on the audio clock so that a fallback to it is seamless.
            self.clock.seek(now, Instant::now());
        }
        let (frame, dropped) = select_frame(&mut self.queue, now);
        self.dropped += dropped as u64;
        self.position = now.min(self.info.duration);
        let frame = frame.map(|f| self.present(f));
        if self.at_end() {
            self.pause();
        }
        frame
    }

    /// Moves messages of the current generation from the channel to the queue.
    fn receive(&mut self) {
        while self.queue.len() < QUEUE_CAP {
            match self.msgs.try_recv() {
                Ok(Msg::Frame {
                    generation,
                    frame,
                    ready,
                }) if generation == self.generation => {
                    if self.awaiting_seek_frame && self.queue.is_empty() {
                        self.seek_frame_ready = Some(ready);
                    }
                    self.queue.push_back(frame)
                }
                Ok(Msg::End { generation }) if generation == self.generation => {
                    self.end_seen = true
                }
                Ok(_) => {} // decoded before the latest seek
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The decode thread is gone: nothing more will come.
                    self.end_seen = true;
                    break;
                }
            }
        }
    }

    /// Falls back to the system clock when the audio runs out (end of the
    /// audio track, or no audio while video is waiting) or the output device
    /// stops consuming samples.
    fn watch_audio(&mut self) {
        if !self.audio_driven() {
            self.stall.reset();
            return;
        }
        let (queued, played) = {
            let a = self.audio.as_ref().unwrap().lock().unwrap();
            (a.queued_frames(), a.frames_played())
        };
        if self.stall.stalled(queued, played, Instant::now()) {
            log::warn!(
                "audio output stalled at {:.3}s, using the system clock from now on",
                self.position
            );
            self.audio_dead = true;
            self.update_audio_mode();
            return;
        }
        if queued > 0 {
            self.starving_since = None;
            return;
        }
        let lost = if self.end_seen {
            true
        } else if self.queue.len() >= QUEUE_CAP {
            let since = *self.starving_since.get_or_insert_with(Instant::now);
            since.elapsed() >= AUDIO_STARVATION
        } else {
            self.starving_since = None;
            false
        };
        if lost {
            if !self.end_seen {
                log::warn!(
                    "audio starved at {:.3}s, using the system clock",
                    self.position
                );
            }
            self.audio_lost = true;
            self.update_audio_mode();
        }
    }

    fn present(&mut self, f: Nv12Frame) -> Nv12Frame {
        self.presented += 1;
        self.last_frame_pts = f.pts;
        f
    }

    fn frame_duration(&self) -> f64 {
        if self.info.video.fps > 0.0 {
            1.0 / self.info.video.fps
        } else {
            1.0 / 30.0
        }
    }

    /// Audio would drive playback if it were playing (ignores a lost track).
    fn audio_can_drive(&self) -> bool {
        self.audio.is_some() && !self.audio_dead && self.speed() == 1.0
    }

    fn audio_driven(&self) -> bool {
        self.audio_can_drive() && !self.audio_lost && !self.is_paused()
    }

    fn current_time(&self) -> f64 {
        if self.audio_driven() {
            self.audio.as_ref().unwrap().lock().unwrap().clock()
        } else {
            self.clock.time(Instant::now())
        }
    }

    fn update_audio_mode(&mut self) {
        let driven = self.audio_driven();
        self.shared.audio_wanted.store(driven, Ordering::SeqCst);
        if let Some(a) = &self.audio {
            a.lock().unwrap().set_muted(!driven);
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Quit);
        if let Some(t) = self.thread.take() {
            // The worker checks its commands at least every POLL, even when
            // the channel or the audio buffer is full.
            let _ = t.join();
        }
    }
}

/// Detects an audio output that stopped consuming samples: samples are
/// queued but the played-frames counter has not moved for
/// [`AUDIO_STARVATION`]. The counter is used rather than the audio clock,
/// which stays at its base for one output latency after every (re)start.
#[derive(Debug, Default)]
struct StallDetector {
    /// Last played-frames count seen and when it was first seen.
    last: Option<(u64, Instant)>,
}

impl StallDetector {
    fn reset(&mut self) {
        self.last = None;
    }

    /// Records one observation while audio drives playback; returns true
    /// once the output is considered stalled.
    fn stalled(&mut self, queued: usize, played: u64, now: Instant) -> bool {
        if queued == 0 {
            // An empty buffer is starvation, handled separately.
            self.last = None;
            return false;
        }
        match self.last {
            Some((seen, since)) if seen == played => {
                now.saturating_duration_since(since) >= AUDIO_STARVATION
            }
            _ => {
                self.last = Some((played, now));
                false
            }
        }
    }
}

/// Per-seek state of the decode thread.
struct Generation {
    id: u64,
    /// Precise seek: decoded frames before this time are not shown.
    skip_before: f64,
    /// Latest skipped frame, shown if the stream ends before `skip_before`.
    last_skipped: Option<Nv12Frame>,
    /// The first frame was accepted and audio aligned to it.
    started: bool,
    /// Media time the audio must start at (the first frame's pts).
    audio_start: f64,
    /// The first audio samples were trimmed or padded to `audio_start`.
    audio_synced: bool,
}

impl Generation {
    fn new(id: u64, skip_before: f64) -> Self {
        Self {
            id,
            skip_before,
            last_skipped: None,
            started: false,
            audio_start: 0.0,
            audio_synced: false,
        }
    }
}

struct Worker {
    path: PathBuf,
    info: MediaInfo,
    prefer_hw: bool,
    audio: Option<Arc<Mutex<AudioOutput>>>,
    commands: Receiver<Command>,
    msgs: Sender<Msg>,
    shared: Arc<Shared>,
}

/// Mutable decode-thread state, reset at every seek.
struct Pipeline {
    generation: Generation,
    /// Demuxed video packets not yet sent to the decoder.
    video_packets: VecDeque<ffmpeg::Packet>,
    /// A message waiting for room on the channel.
    outbox: Option<Msg>,
    /// Audio decoded before the first frame of the generation was known.
    early_audio: Vec<AudioChunk>,
    /// Interleaved samples waiting for room in the audio output.
    audio_buf: Vec<f32>,
    audio_off: usize,
    demux_eof: bool,
    video_eof_sent: bool,
    end_sent: bool,
}

impl Pipeline {
    fn new(generation: Generation) -> Self {
        Self {
            generation,
            video_packets: VecDeque::new(),
            outbox: None,
            early_audio: Vec::new(),
            audio_buf: Vec::new(),
            audio_off: 0,
            demux_eof: false,
            video_eof_sent: false,
            end_sent: false,
        }
    }

    fn audio_pending(&self) -> bool {
        self.audio_off < self.audio_buf.len()
    }

    fn drop_audio(&mut self) {
        self.early_audio.clear();
        self.audio_buf.clear();
        self.audio_off = 0;
    }
}

impl Worker {
    fn run(self) -> Result<(), MediaError> {
        let mut input = ffmpeg::format::input(&self.path)?;
        let vindex = self.info.video.stream_index;
        let vstream = input.stream(vindex).ok_or(MediaError::NoVideoStream)?;
        let mut video = VideoDecoder::open(
            vstream.parameters(),
            self.info.video.time_base,
            self.prefer_hw,
        )?;
        let mut audio_dec = match (&self.audio, &self.info.audio) {
            (Some(out), Some(a)) => {
                let s = input
                    .stream(a.stream_index)
                    .ok_or_else(|| MediaError::Audio("audio stream vanished".into()))?;
                let rate = out.lock().unwrap().sample_rate();
                let dec = AudioDecoder::open(s.parameters(), f64::from(s.time_base()), rate)?;
                Some((a.stream_index, dec, rate))
            }
            _ => None,
        };
        let half_frame = 0.5 / self.info.video.fps.max(1.0);

        let mut p = Pipeline::new(Generation::new(0, f64::NEG_INFINITY));
        let mut next_command: Option<Command> = None;

        loop {
            // 1. Commands: only the latest seek matters, quit wins.
            let mut seek = None;
            let mut command = next_command.take();
            loop {
                match command {
                    Some(Command::Quit) => return Ok(()),
                    Some(Command::Seek {
                        to,
                        precise,
                        generation,
                    }) => seek = Some((to, precise, generation)),
                    None => {}
                }
                command = match self.commands.try_recv() {
                    Ok(c) => Some(c),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                };
            }
            if let Some((to, precise, generation)) = seek {
                let ts = (to * f64::from(ffmpeg::ffi::AV_TIME_BASE)) as i64;
                if let Err(e) = input.seek(ts, ..ts) {
                    log::warn!("seek to {to:.3}s failed: {e}");
                }
                video.flush();
                if let Some((_, a, _)) = &mut audio_dec {
                    a.flush();
                }
                let skip_before = if precise {
                    to - half_frame
                } else {
                    f64::NEG_INFINITY
                };
                p = Pipeline::new(Generation::new(generation, skip_before));
            }

            let mut progressed = false;
            let audio_wanted =
                audio_dec.is_some() && self.shared.audio_wanted.load(Ordering::SeqCst);
            if !audio_wanted {
                p.drop_audio();
            }

            // 2. Deliver the pending message.
            if let Some(msg) = p.outbox.take() {
                match self.msgs.try_send(msg) {
                    Ok(()) => progressed = true,
                    Err(TrySendError::Full(msg)) => p.outbox = Some(msg),
                    Err(TrySendError::Disconnected(_)) => return Ok(()),
                }
            }

            // 3. Feed the audio output.
            if p.audio_pending() {
                let pushed = self.push_audio(p.generation.id, &p.audio_buf[p.audio_off..]);
                p.audio_off += pushed;
                progressed |= pushed > 0;
                if !p.audio_pending() {
                    p.audio_buf.clear();
                    p.audio_off = 0;
                }
            }

            // 4. Decode the next video frame when there is room for it.
            let mut need_video_packet = false;
            if p.outbox.is_none() && !p.end_sent {
                let decoded = video.receive().unwrap_or_else(|e| {
                    log::warn!("video decoding error: {e}");
                    None
                });
                if let Some(frame) = decoded {
                    progressed = true;
                    *self.shared.backend.lock().unwrap() = video.active_backend();
                    if let Some(frame) = self.accept(&mut p, frame, audio_dec.as_ref().map(|a| a.2))
                    {
                        p.outbox = Some(Msg::Frame {
                            generation: p.generation.id,
                            frame,
                            ready: Instant::now(),
                        });
                    }
                } else if let Some(packet) = p.video_packets.pop_front() {
                    progressed = true;
                    if let Err(e) = video.send(&packet) {
                        log::warn!("video packet rejected: {e}");
                    }
                } else if p.demux_eof && !p.video_eof_sent {
                    progressed = true;
                    p.video_eof_sent = true;
                    if let Err(e) = video.send_eof() {
                        log::warn!("video decoder drain failed: {e}");
                    }
                } else if p.video_eof_sent {
                    // Fully drained.
                    if !p.generation.started
                        && let Some(frame) = p.generation.last_skipped.take()
                    {
                        // The precise target was past the last frame: show the last one.
                        progressed = true;
                        p.generation.skip_before = f64::NEG_INFINITY;
                        let frame = self.accept(&mut p, frame, audio_dec.as_ref().map(|a| a.2));
                        p.outbox = frame.map(|frame| Msg::Frame {
                            generation: p.generation.id,
                            frame,
                            ready: Instant::now(),
                        });
                    } else if !p.audio_pending() {
                        progressed = true;
                        p.end_sent = true;
                        p.outbox = Some(Msg::End {
                            generation: p.generation.id,
                        });
                    }
                } else {
                    need_video_packet = true;
                }
            }

            // 5. Demux when video needs data, or when the audio output is
            //    running low (reading video packets ahead, compressed).
            let audio_hungry = audio_wanted
                && p.generation.started
                && !p.audio_pending()
                && p.video_packets.len() < MAX_VIDEO_PACKETS
                && self.audio_has_room();
            if !p.demux_eof && (need_video_packet || audio_hungry) {
                progressed = true;
                match input.packets().next() {
                    None => p.demux_eof = true,
                    Some((stream, packet)) => {
                        if stream.index() == vindex {
                            p.video_packets.push_back(packet);
                        } else if let Some((aindex, dec, rate)) = &mut audio_dec
                            && stream.index() == *aindex
                            && audio_wanted
                        {
                            if let Err(e) = dec.send(&packet) {
                                log::warn!("audio packet rejected: {e}");
                            }
                            loop {
                                match dec.receive() {
                                    Ok(Some(chunk)) => queue_audio(&mut p, chunk, *rate),
                                    Ok(None) => break,
                                    Err(e) => {
                                        log::warn!("audio decoding error: {e}");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 6. Nothing to do right now: wait, but keep listening for commands.
            if !progressed {
                let idle = p.end_sent && p.outbox.is_none() && !p.audio_pending();
                let received = if idle {
                    self.commands
                        .recv()
                        .map_err(|_| RecvTimeoutError::Disconnected)
                } else {
                    self.commands.recv_timeout(POLL)
                };
                match received {
                    Ok(c) => next_command = Some(c),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return Ok(()),
                }
            }
        }
    }

    /// Applies the precise-seek skip; on the generation's first frame, aligns
    /// the audio to it. Returns the frame if it must be shown.
    fn accept(&self, p: &mut Pipeline, frame: Nv12Frame, rate: Option<u32>) -> Option<Nv12Frame> {
        let g = &mut p.generation;
        if !g.started {
            if frame.pts < g.skip_before {
                g.last_skipped = Some(frame);
                return None;
            }
            g.started = true;
            g.last_skipped = None;
            g.audio_start = frame.pts;
            let start_ok = self.with_audio(g.id, |a| a.reset(frame.pts)).is_some();
            if let Some(rate) = rate
                && start_ok
            {
                for chunk in std::mem::take(&mut p.early_audio) {
                    queue_audio(p, chunk, rate);
                }
            } else {
                p.early_audio.clear();
            }
        }
        Some(frame)
    }

    /// Runs `f` on the audio output if `generation` is still the latest one.
    /// The check happens under the audio lock, which the player also holds
    /// while resetting the output on seek.
    fn with_audio<R>(&self, generation: u64, f: impl FnOnce(&mut AudioOutput) -> R) -> Option<R> {
        let mut a = self.audio.as_ref()?.lock().unwrap();
        (self.shared.generation.load(Ordering::SeqCst) == generation).then(|| f(&mut a))
    }

    fn push_audio(&self, generation: u64, samples: &[f32]) -> usize {
        self.with_audio(generation, |a| a.push(samples))
            .unwrap_or(0)
    }

    fn audio_has_room(&self) -> bool {
        self.audio.as_ref().is_some_and(|a| {
            let a = a.lock().unwrap();
            let capacity = f64::from(a.sample_rate()) * AudioOutput::CAPACITY_SECONDS;
            (a.queued_frames() as f64) < capacity * 0.75
        })
    }
}

/// Queues decoded audio for the output. Before the generation's first frame is
/// known the chunk is kept aside; the first samples after it are trimmed (or
/// padded with silence) so the audio starts exactly at the first frame's pts.
fn queue_audio(p: &mut Pipeline, chunk: AudioChunk, rate: u32) {
    let g = &mut p.generation;
    let rate_f = f64::from(rate);
    let frames = chunk.samples.len() / 2;
    if !g.started {
        // Drop audio that certainly ends before a precise target.
        if chunk.pts + frames as f64 / rate_f >= g.skip_before - 0.1 {
            p.early_audio.push(chunk);
        }
        return;
    }
    if g.audio_synced {
        p.audio_buf.extend_from_slice(&chunk.samples);
        return;
    }
    let skip = ((g.audio_start - chunk.pts) * rate_f).round() as i64;
    if skip >= frames as i64 {
        return; // entirely before the start
    }
    if skip >= 0 {
        p.audio_buf
            .extend_from_slice(&chunk.samples[skip as usize * 2..]);
    } else {
        let pad = (-skip).min(i64::from(rate)) as usize;
        p.audio_buf.resize(p.audio_buf.len() + pad * 2, 0.0);
        p.audio_buf.extend_from_slice(&chunk.samples);
    }
    g.audio_synced = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::audio_clock_time;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn consuming_device_is_not_stalled() {
        let t0 = Instant::now();
        let mut d = StallDetector::default();
        for i in 0..100u32 {
            assert!(!d.stalled(4800, u64::from(i) * 480, t0 + i * 10 * MS));
        }
    }

    #[test]
    fn frozen_counter_with_queued_samples_is_stalled() {
        let t0 = Instant::now();
        let mut d = StallDetector::default();
        assert!(!d.stalled(4800, 48_000, t0));
        assert!(!d.stalled(4800, 48_000, t0 + 499 * MS));
        assert!(d.stalled(4800, 48_000, t0 + 500 * MS));
    }

    #[test]
    fn empty_buffer_or_reset_restarts_the_wait() {
        let t0 = Instant::now();
        let mut d = StallDetector::default();
        assert!(!d.stalled(4800, 100, t0));
        // starvation (empty buffer) is not a stall, and restarts the wait
        assert!(!d.stalled(0, 100, t0 + 400 * MS));
        assert!(!d.stalled(4800, 100, t0 + 600 * MS));
        assert!(!d.stalled(4800, 100, t0 + 1000 * MS));
        assert!(d.stalled(4800, 100, t0 + 1100 * MS));
        d.reset();
        assert!(!d.stalled(4800, 100, t0 + 2000 * MS));
    }

    #[test]
    fn counter_moving_again_restarts_the_wait() {
        let t0 = Instant::now();
        let mut d = StallDetector::default();
        assert!(!d.stalled(4800, 100, t0));
        assert!(!d.stalled(4800, 580, t0 + 450 * MS));
        assert!(!d.stalled(4800, 580, t0 + 900 * MS));
        assert!(d.stalled(4800, 580, t0 + 950 * MS));
    }

    #[test]
    fn high_latency_device_is_not_stalled_while_its_clock_is_pinned() {
        // 800 ms output latency at 48 kHz: for the first 800 ms after a start
        // the audio clock stays at its base, but frames are being consumed.
        let t0 = Instant::now();
        let latency = Duration::from_millis(800);
        let mut d = StallDetector::default();
        for i in 0..=100u32 {
            let elapsed = i * 10 * MS;
            let played = u64::from(i) * 480;
            let clock = audio_clock_time(5.0, played, 48_000, latency);
            if elapsed < latency {
                assert_eq!(clock, 5.0, "clock should be pinned at base");
            }
            assert!(
                !d.stalled(4800, played, t0 + elapsed),
                "false stall at {elapsed:?}"
            );
        }
    }
}
