//! Demuxing of the GoPro metadata stream (GPMF, codec tag `gpmd`).
use std::path::Path;

use ffmpeg_next as ffmpeg;

use crate::{MediaError, ffmpeg_info};

/// One packet of the metadata stream. Times are seconds of file time.
#[derive(Debug, Clone, PartialEq)]
pub struct GpmfPacket {
    pub pts: f64,
    pub duration: f64,
    pub data: Vec<u8>,
}

const GPMD_TAG: u32 = u32::from_le_bytes(*b"gpmd");

/// Consecutive unreadable packets after which the read gives up.
const MAX_INVALID_IN_A_ROW: usize = 1000;

/// Lowers FFmpeg's log level to Error while alive and restores the previous
/// level on drop. Opening a GoPro file makes FFmpeg warn about the zero
/// duration of the `fdsc` stream, which is harmless.
///
/// FFmpeg's log level is a single process-wide global: another thread that
/// sets it (or logs) while this guard is alive sees Error, and two
/// overlapping guards may restore in the wrong order. ActionLay only changes
/// the level here and in `ffmpeg_info::init`, so the worst case is a hidden
/// warning or a briefly wrong level, never a crash.
pub(crate) struct QuietLog(ffmpeg::util::log::Level);

impl QuietLog {
    pub(crate) fn new() -> Self {
        use ffmpeg::util::log;
        // get_level fails only for a level FFmpeg itself never sets.
        let previous = log::get_level().unwrap_or(log::Level::Warning);
        log::set_level(log::Level::Error);
        QuietLog(previous)
    }
}

impl Drop for QuietLog {
    fn drop(&mut self) {
        ffmpeg::util::log::set_level(self.0);
    }
}

/// Reads every packet of the GoPro metadata stream (codec tag `gpmd`,
/// handler "GoPro MET"). `Ok(vec![])` when the file has no such stream.
///
/// Corrupt packets are skipped (up to 1000 in a row, then the read stops
/// with what it has). A read error in the middle of the file (e.g.
/// a truncated recording) is logged and the packets read so far are returned
/// instead of an error: partial telemetry is better than none.
pub fn read_gpmf_packets(path: &Path) -> Result<Vec<GpmfPacket>, MediaError> {
    read_gpmf_range(
        path,
        0.0,
        f64::INFINITY,
        &std::sync::atomic::AtomicBool::new(false),
    )
}

pub fn read_gpmf_packets_with_cancel(
    path: &Path,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<Vec<GpmfPacket>, MediaError> {
    read_gpmf_range(path, 0.0, f64::INFINITY, &cancel)
}

/// Complete metadata for route exports: refuse read failures or missing packets.
pub fn read_gpmf_packets_complete(
    path: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<GpmfPacket>, MediaError> {
    let mut input = open_metadata(path, cancelled)?;
    read_range_input_policy(&mut input, 0.0, f64::INFINITY, cancelled, true)
        .map(|(packets, _)| packets)
}

/// Reads only the metadata stream in a time range. MP4's sample index lets the
/// demuxer seek directly to metadata; video/audio streams are discarded.
/// Includes the metadata packet before `start` for continuous interpolation.
pub fn read_gpmf_range(
    path: &Path,
    start: f64,
    end: f64,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<GpmfPacket>, MediaError> {
    read_gpmf_range_measured(path, start, end, cancelled).map(|(packets, _)| packets)
}

// AVIO's byte counter measures demux reads, including MP4 headers and buffering.
fn read_gpmf_range_measured(
    path: &Path,
    start: f64,
    end: f64,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<(Vec<GpmfPacket>, i64), MediaError> {
    let mut input = open_metadata(path, cancelled)?;
    read_range_input(&mut input, start, end, cancelled)
}

fn open_metadata(
    path: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<ffmpeg::format::context::Input, MediaError> {
    ffmpeg_info::init();
    let _quiet = QuietLog::new();
    // MP4 stores gpmd stream descriptors and its sample index in the header.
    // Avoid find_stream_info: it probes media payloads we do not need here.
    let name = std::ffi::CString::new(path.to_string_lossy().as_bytes())
        .map_err(|e| MediaError::Io(e.to_string()))?;
    unsafe extern "C" fn interrupt(opaque: *mut std::ffi::c_void) -> i32 {
        // SAFETY: the borrowed AtomicBool outlives this synchronous input context.
        unsafe {
            (&*(opaque as *const std::sync::atomic::AtomicBool))
                .load(std::sync::atomic::Ordering::Relaxed) as i32
        }
    }
    // SAFETY: allocated context belongs to Input after opening successfully;
    // avformat_open_input frees it on failure. Callback's borrow stays valid
    // until Input drops, including while opening and seeking on slow storage.
    let input = unsafe {
        let mut raw = ffmpeg::ffi::avformat_alloc_context();
        if raw.is_null() {
            return Err(MediaError::Io("cannot allocate metadata input".into()));
        }
        (*raw).interrupt_callback = ffmpeg::ffi::AVIOInterruptCB {
            callback: Some(interrupt),
            opaque: cancelled as *const _ as *mut std::ffi::c_void,
        };
        let result = ffmpeg::ffi::avformat_open_input(
            &mut raw,
            name.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
        // Header parsing benefits from buffered reads. Afterwards metadata packets
        // are small and scattered between large media chunks: direct AVIO reads
        // avoid fetching a 32 KiB buffer for each ~4 KiB metadata packet.
        let pb = (*raw).pb;
        if !pb.is_null() {
            (*pb).direct = 1;
        }
        ffmpeg::format::context::Input::wrap(raw)
    };
    Ok(input)
}

fn read_range_input(
    input: &mut ffmpeg::format::context::Input,
    start: f64,
    end: f64,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<(Vec<GpmfPacket>, i64), MediaError> {
    read_range_input_policy(input, start, end, cancelled, false)
}

fn read_range_input_policy(
    input: &mut ffmpeg::format::context::Input,
    start: f64,
    end: f64,
    cancelled: &std::sync::atomic::AtomicBool,
    require_complete: bool,
) -> Result<(Vec<GpmfPacket>, i64), MediaError> {
    let Some(index) = input.streams().find(is_gpmd).map(|s| s.index()) else {
        return Ok((Vec::new(), 0));
    };
    let time_base = f64::from(
        input
            .stream(index)
            .map_or(ffmpeg::Rational::new(1, 1000), |s| s.time_base()),
    );
    // Discarded streams are skipped by the demuxer without reading their
    // payload, so a multi-GB video costs only its metadata bytes.
    for i in 0..input.nb_streams() as usize {
        if i != index {
            // SAFETY: i < nb_streams, and the AVStream array lives as long
            // as `input`; only the `discard` field is written.
            unsafe {
                let stream = *(*input.as_mut_ptr()).streams.add(i);
                (*stream).discard = ffmpeg::ffi::AVDiscard::AVDISCARD_ALL;
            }
        }
    }
    if start >= 0.0 && start.is_finite() {
        // SAFETY: live input context, valid metadata stream index and stream timebase.
        let result = unsafe {
            ffmpeg::ffi::av_seek_frame(
                input.as_mut_ptr(),
                index as i32,
                (start / time_base) as i64,
                ffmpeg::ffi::AVSEEK_FLAG_BACKWARD,
            )
        };
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
    }
    let mut packets = Vec::new();
    let mut packet = ffmpeg::Packet::empty();
    let mut invalid_in_a_row = 0;
    loop {
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        match packet.read(input) {
            Ok(()) => invalid_in_a_row = 0,
            Err(ffmpeg::Error::Eof) => break,
            Err(ffmpeg::Error::InvalidData) => {
                if require_complete {
                    return Err(ffmpeg::Error::InvalidData.into());
                }
                invalid_in_a_row += 1;
                if invalid_in_a_row >= MAX_INVALID_IN_A_ROW {
                    log::warn!(
                        "gpmd read stopped after {invalid_in_a_row} unreadable packets in a row, \
                         returning the {} read",
                        packets.len()
                    );
                    break;
                }
                continue;
            }
            Err(e) => {
                if require_complete {
                    return Err(e.into());
                }
                log::warn!(
                    "gpmd read stopped by error after {} packets, returning them: {e}",
                    packets.len()
                );
                break;
            }
        }
        if packet.stream() != index {
            continue;
        }
        let (Some(ts), Some(data)) = (packet.pts().or(packet.dts()), packet.data()) else {
            if require_complete {
                return Err(MediaError::Io(
                    "metadata packet has no timestamp or data".into(),
                ));
            }
            log::warn!("gpmd packet without timestamp or data skipped");
            continue;
        };
        if ts as f64 * time_base > end {
            break;
        }
        packets.push(GpmfPacket {
            pts: ts as f64 * time_base,
            duration: packet.duration().max(0) as f64 * time_base,
            data: data.to_vec(),
        });
        // The MP4 index tells us when the next metadata packet is beyond the
        // requested range; avoid reading that extra payload just to detect it.
        // SAFETY: metadata stream belongs to the live context; the index entry
        // is only inspected while the demuxer is idle and remains unmodified.
        if end.is_finite() {
            let next_is_outside = unsafe {
                let stream = *(*input.as_mut_ptr()).streams.add(index);
                let next = ffmpeg::ffi::avformat_index_get_entry_from_timestamp(
                    stream,
                    packet.dts().unwrap_or(ts).saturating_add(1),
                    ffmpeg::ffi::AVSEEK_FLAG_ANY,
                );
                !next.is_null() && (*next).timestamp as f64 * time_base > end
            };
            if next_is_outside {
                break;
            }
        }
    }
    fill_missing_durations(&mut packets);
    // SAFETY: the input and its AVIO context are still alive.
    let bytes = unsafe {
        let pb = (*input.as_ptr()).pb;
        if pb.is_null() { 0 } else { (*pb).bytes_read }
    };
    if require_complete && !cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        let expected = input.stream(index).map_or(0, |stream| stream.frames());
        if expected > 0 && packets.len() != expected as usize {
            return Err(MediaError::Io(format!(
                "incomplete metadata: read {} of {expected} packets",
                packets.len()
            )));
        }
    }
    Ok((packets, bytes))
}

/// Reuses the MP4 header and metadata sample index across route seeks.
/// Kept on the metadata worker, separately from the playback decoder.
pub struct GpmfReader {
    // Drop Input before the Arc backing its interrupt callback.
    input: ffmpeg::format::context::Input,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl GpmfReader {
    pub fn open(
        path: &Path,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<Self, MediaError> {
        let input = open_metadata(path, &cancelled)?;
        Ok(Self { input, cancelled })
    }
    /// Duration recorded in the container header, without probing media payloads.
    pub fn duration(&self) -> f64 {
        let container = self.input.duration().max(0) as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE);
        self.input
            .streams()
            .map(|stream| stream.duration().max(0) as f64 * f64::from(stream.time_base()))
            .filter(|duration| duration.is_finite())
            .fold(container, f64::max)
    }

    /// Bytes fetched by FFmpeg's AVIO layer, including headers and buffering.
    /// Filesystem/cloud-client read-ahead can transfer more bytes over the network.
    pub fn bytes_read(&self) -> u64 {
        // SAFETY: Input owns the live AVIO context.
        unsafe {
            let pb = (*self.input.as_ptr()).pb;
            if pb.is_null() {
                0
            } else {
                (*pb).bytes_read.max(0) as u64
            }
        }
    }

    pub fn read_range(&mut self, start: f64, end: f64) -> Result<Vec<GpmfPacket>, MediaError> {
        read_range_input(&mut self.input, start, end, &self.cancelled).map(|(packets, _)| packets)
    }

    /// Validate EOF and packet count before declaring a complete route ready.
    pub fn read_complete(&mut self) -> Result<Vec<GpmfPacket>, MediaError> {
        read_range_input_policy(&mut self.input, 0.0, f64::INFINITY, &self.cancelled, true)
            .map(|(packets, _)| packets)
    }
}

pub(crate) fn is_gpmd(stream: &ffmpeg::format::stream::Stream) -> bool {
    let params = stream.parameters();
    // SAFETY: the parameters belong to a live stream; codec_tag is plain data.
    let tag = unsafe { (*params.as_ptr()).codec_tag };
    tag == GPMD_TAG
        || stream
            .metadata()
            .get("handler_name")
            .is_some_and(|h| h.trim() == "GoPro MET")
}

/// Packets the muxer stored without a duration get the gap to the next
/// packet; a last packet without one gets the previous packet's duration.
pub fn fill_missing_durations(packets: &mut [GpmfPacket]) {
    for i in 0..packets.len() {
        if packets[i].duration > 0.0 {
            continue;
        }
        packets[i].duration = if i + 1 < packets.len() {
            (packets[i + 1].pts - packets[i].pts).max(0.0)
        } else if i > 0 {
            packets[i - 1].duration
        } else {
            0.0
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pts: f64, duration: f64) -> GpmfPacket {
        GpmfPacket {
            pts,
            duration,
            data: Vec::new(),
        }
    }

    #[test]
    fn missing_durations_come_from_neighbours() {
        let mut v = vec![p(0.0, 0.0), p(1.0, 1.0), p(2.0, 0.0)];
        fill_missing_durations(&mut v);
        let d: Vec<f64> = v.iter().map(|x| x.duration).collect();
        assert_eq!(d, vec![1.0, 1.0, 1.0]);
        let mut one = vec![p(0.0, 0.0)];
        fill_missing_durations(&mut one);
        assert_eq!(one[0].duration, 0.0);
    }

    #[test]
    fn gpmd_tag_value() {
        assert_eq!(GPMD_TAG, 0x646d_7067);
    }
}

#[cfg(test)]
mod indexed_io_tests {
    use super::*;
    #[test]
    fn metadata_seek_does_not_read_the_video_payload() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/gopro");
        for name in [
            "hero5.mp4",
            "hero6.mp4",
            "hero7.mp4",
            "hero8.mp4",
            "max-heromode.mp4",
        ] {
            let path = root.join(name);
            if !path.exists() {
                continue;
            }
            let size = std::fs::metadata(&path).unwrap().len();
            let (full, all_bytes) = read_gpmf_range_measured(
                &path,
                0.0,
                f64::INFINITY,
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
            let start = full[full.len() / 2].pts;
            let (part, range_bytes) = read_gpmf_range_measured(
                &path,
                start,
                start + 1.0,
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
            eprintln!("{name}: file {size}, metadata all {all_bytes}, range {range_bytes}");
            assert!(
                all_bytes > 0 && (all_bytes as u64) < size / 4,
                "full metadata read must skip media payload: {name}"
            );
            assert!(range_bytes <= all_bytes && part.len() <= 3, "{name}");
            let mut reader = GpmfReader::open(
                &path,
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )
            .unwrap();
            assert!(reader.duration() > 0.0);
            let header_bytes = reader.bytes_read();
            let first = reader.read_range(start, start + 1.0).unwrap();
            let payload = first
                .iter()
                .map(|packet| packet.data.len() as u64)
                .sum::<u64>();
            assert!(
                reader.bytes_read() - header_bytes <= payload + 1024,
                "metadata I/O amplification: {name}"
            );
            let before = reader.bytes_read();
            assert_eq!(reader.read_range(start, start + 1.0).unwrap(), first);
            assert!(
                reader.bytes_read() - before <= payload + 1024,
                "header must be retained across seeks: {name}"
            );
            let before = reader.bytes_read();
            assert_eq!(reader.read_complete().unwrap(), full);
            assert!(
                reader.bytes_read() - before < size / 4,
                "validated full-route read must skip video/audio payload: {name}"
            );
        }
    }
}
