//! The I/O thread owns the demuxer. A slow OS read never stalls decoding of
//! packets already buffered. Seek generations discard obsolete deliveries.
use super::*;
pub(super) enum PacketMessage {
    Packet {
        generation: u64,
        stream: usize,
        packet: ffmpeg::Packet,
    },
    End {
        generation: u64,
    },
}
pub(super) fn spawn(
    mut input: ffmpeg::format::context::Input,
    info: MediaInfo,
    shared: Arc<Shared>,
    metadata: std::sync::mpsc::Sender<TelemetryEvent>,
) -> (Sender<Command>, Receiver<PacketMessage>) {
    let (commands, receive) = unbounded();
    let (send, packets) = bounded(MAX_VIDEO_PACKETS);
    std::thread::Builder::new()
        .name("actionlay-read".into())
        .spawn(move || {
            let mut generation = 0;
            let mut from_start = true;
            let mut eof = false;
            let mut metadata_started = info.telemetry.is_none();
            let mut pending: Option<PacketMessage> = None;
            loop {
                if shared.cancelled.load(Ordering::SeqCst) {
                    return;
                }
                let mut seek = None;
                loop {
                    match receive.try_recv() {
                        Ok(Command::Quit) | Err(TryRecvError::Disconnected) => return,
                        Ok(Command::Seek {
                            to,
                            precise: _,
                            generation: g,
                        }) => seek = Some((to, g)),
                        Err(TryRecvError::Empty) => break,
                    }
                }
                if let Some((to, g)) = seek {
                    if let Some(PacketMessage::Packet { packet, .. }) = pending.take() {
                        shared
                            .packet_bytes
                            .fetch_sub(packet.size(), Ordering::SeqCst);
                    }
                    generation = g;
                    from_start = to <= 0.001;
                    eof = false;
                    metadata_started = info.telemetry.is_none();
                    let ts = (to * f64::from(ffmpeg::ffi::AV_TIME_BASE)) as i64;
                    if let Err(e) = input.seek(ts, ..ts) {
                        log::warn!("seek to {to:.3}s failed: {e}");
                    }
                }
                if let Some(message) = pending.take() {
                    match send.try_send(message) {
                        Ok(()) => {}
                        Err(TrySendError::Full(message)) => pending = Some(message),
                        Err(TrySendError::Disconnected(_)) => return,
                    }
                }
                let current = shared.generation.load(Ordering::SeqCst) == generation;
                let full = shared.packet_bytes.load(Ordering::SeqCst)
                    >= shared.buffering.lock().unwrap().packet_bytes()
                    || pending.is_some();
                let mut buffer = shared.buffer.lock().unwrap();
                if buffer.generation == generation {
                    buffer.full = full;
                }
                let ahead =
                    buffer.until >= f64::from_bits(shared.read_until.load(Ordering::SeqCst));
                drop(buffer);
                if !current || full || eof || (ahead && metadata_started) {
                    std::thread::sleep(POLL);
                    continue;
                }
                match input.packets().next() {
                    None => {
                        eof = true;
                        if shared.generation.load(Ordering::SeqCst) == generation {
                            let mut buffer = shared.buffer.lock().unwrap();
                            if buffer.generation == generation {
                                buffer.eof = true;
                            }
                        }
                        let _ = metadata.send(TelemetryEvent::End { from_start });
                        pending = Some(PacketMessage::End { generation });
                    }
                    Some((stream, packet)) => {
                        let index = stream.index();
                        if let Some(meta) = &info.telemetry
                            && index == meta.stream_index
                        {
                            if let (Some(timestamp), Some(data)) =
                                (packet.pts().or(packet.dts()), packet.data())
                            {
                                metadata_started = true;
                                let _ = metadata.send(TelemetryEvent::Packet {
                                    timestamp,
                                    packet: GpmfPacket {
                                        pts: timestamp as f64 * meta.time_base,
                                        duration: packet.duration().max(0) as f64 * meta.time_base,
                                        data: data.to_vec(),
                                    },
                                });
                            }
                        } else if index == info.video.stream_index
                            || info.audio.as_ref().is_some_and(|a| a.stream_index == index)
                        {
                            if index == info.video.stream_index
                                && shared.generation.load(Ordering::SeqCst) == generation
                                && let Some(pts) = packet.pts().or(packet.dts())
                            {
                                let until = (pts + packet.duration()) as f64 * info.video.time_base;
                                let mut buffer = shared.buffer.lock().unwrap();
                                if buffer.generation == generation {
                                    buffer.until = buffer.until.max(until);
                                }
                            }
                            shared
                                .packet_bytes
                                .fetch_add(packet.size(), Ordering::SeqCst);
                            pending = Some(PacketMessage::Packet {
                                generation,
                                stream: index,
                                packet,
                            });
                        }
                    }
                }
            }
        })
        .expect("spawn read thread");
    (commands, packets)
}
