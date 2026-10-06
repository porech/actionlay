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
struct QuietLog(ffmpeg::util::log::Level);

impl QuietLog {
    fn new() -> Self {
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
    read_gpmf_packets_with_cancel(
        path,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
}

pub fn read_gpmf_packets_with_cancel(
    path: &Path,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<Vec<GpmfPacket>, MediaError> {
    ffmpeg_info::init();
    let _quiet = QuietLog::new();
    let mut input = crate::input::open(path, cancel.clone())?;
    let Some(index) = input.streams().find(is_gpmd).map(|s| s.index()) else {
        return Ok(Vec::new());
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
    let mut packets = Vec::new();
    let mut packet = ffmpeg::Packet::empty();
    let mut invalid_in_a_row = 0;
    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        match packet.read(&mut input) {
            Ok(()) => invalid_in_a_row = 0,
            Err(ffmpeg::Error::Eof) => break,
            Err(ffmpeg::Error::InvalidData) => {
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
            log::warn!("gpmd packet without timestamp or data skipped");
            continue;
        };
        packets.push(GpmfPacket {
            pts: ts as f64 * time_base,
            duration: packet.duration().max(0) as f64 * time_base,
            data: data.to_vec(),
        });
    }
    fill_missing_durations(&mut packets);
    Ok(packets)
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
