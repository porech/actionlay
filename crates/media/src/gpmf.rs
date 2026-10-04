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

/// Reads every packet of the GoPro metadata stream (codec tag `gpmd`,
/// handler "GoPro MET"). `Ok(vec![])` when the file has no such stream.
pub fn read_gpmf_packets(path: &Path) -> Result<Vec<GpmfPacket>, MediaError> {
    ffmpeg_info::init();
    let mut input = ffmpeg::format::input(path)?;
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
    for (stream, packet) in input.packets() {
        if stream.index() != index {
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

fn is_gpmd(stream: &ffmpeg::format::stream::Stream) -> bool {
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
