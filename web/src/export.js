import { Input, BlobSource, ALL_FORMATS, VideoSampleSink, EncodedPacketSink, Output, Mp4OutputFormat, CanvasSource, EncodedAudioPacketSource, BufferTarget, StreamTarget, canEncodeVideo } from 'mediabunny';
import { drawOverlay } from './overlay.js';
const MAX_MEMORY_OUTPUT = 256 * 1024 * 1024;

export async function exportVideo(file, core, options, signal, progress) {
  const input = new Input({ source: new BlobSource(file), formats: ALL_FORMATS });
  let output;
  let writable;
  try {
    const track = await input.getPrimaryVideoTrack();
    if (!track || !await track.canDecode()) throw new Error('This browser cannot decode the source video.');
    const duration = await input.computeDuration();
    const start = options.start;
    const end = Math.min(options.end ?? duration, duration);
    if (!Number.isFinite(start) || !Number.isFinite(end) || start < 0 || end <= start) throw new Error('Choose a valid export range within the video.');
    const width = Math.ceil(track.displayWidth / 2) * 2;
    const height = Math.ceil(track.displayHeight / 2) * 2;
    const bitrate = Math.max(2_000_000, Math.min(40_000_000, width * height * 5));
    if (!await canEncodeVideo(options.codec, { width, height, bitrate })) throw new Error('The selected encoder is unavailable at this resolution. Try H.264 or another browser.');
    signal.throwIfAborted();
    let target;
    if (options.handle) {
      writable = await options.handle.createWritable();
      // Keep the filesystem transaction open until the export really succeeds.
      // StreamTarget closes its stream on cancellation as well as finalization.
      target = new StreamTarget(new WritableStream({ write: chunk => writable.write(chunk) }), { chunked: true });
    } else target = new BufferTarget();
    if (!options.handle) target.on('write', ({ end }) => { if (end > MAX_MEMORY_OUTPUT) throw new Error('Export exceeds the 256 MB download buffer. Use a browser with direct file saving or export a shorter range.'); });
    output = new Output({ format: new Mp4OutputFormat({ fastStart: 'fragmented' }), target });
    const canvas = new OffscreenCanvas(width, height);
    const overlay = new OffscreenCanvas(width, height);
    const context = canvas.getContext('2d', { alpha: false });
    const overlayContext = overlay.getContext('2d');
    const videoSource = new CanvasSource(canvas, { codec: options.codec, bitrate });
    output.addVideoTrack(videoSource);
    const audioTrack = options.mode === 'video' ? await input.getPrimaryAudioTrack() : null;
    let audioSource, audioIterator, nextAudio, audioConfig;
    if (audioTrack) {
      const codec = await audioTrack.getCodec();
      if (codec !== 'aac') throw new Error('This first web export supports AAC audio. Export an overlay or use the desktop app for this audio format.');
      audioConfig = await audioTrack.getDecoderConfig();
      audioSource = new EncodedAudioPacketSource(codec);
      output.addAudioTrack(audioSource);
      audioIterator = new EncodedPacketSink(audioTrack).packets();
      nextAudio = await audioIterator.next();
    }
    await output.start();
    let frames = 0;
    const sink = new VideoSampleSink(track);
    for await (const sample of sink.samples(start, end)) {
      try {
        signal.throwIfAborted();
        const timestamp = Math.max(start, sample.timestamp);
        const frameEnd = Math.min(end, sample.timestamp + sample.duration);
        if (frameEnd <= timestamp) continue;
        context.fillStyle = options.mode === 'solid' ? options.color : '#000000';
        context.fillRect(0, 0, width, height);
        if (options.mode === 'video') sample.draw(context, 0, 0, width, height);
        drawOverlay(core, overlayContext, timestamp, width, height);
        // Allow requested tiles to arrive before committing this frame.
        const deadline = performance.now() + 10_000;
        while (core.pending_maps() && performance.now() < deadline) {
          await new Promise(resolve => setTimeout(resolve, 50));
          signal.throwIfAborted();
          drawOverlay(core, overlayContext, timestamp, width, height);
        }
        if (core.pending_maps()) throw new Error('Map tiles did not finish loading. Disable map downloads or retry the export.');
        context.drawImage(overlay, 0, 0);
        await videoSource.add(timestamp - start, frameEnd - timestamp);
        while (nextAudio && !nextAudio.done && nextAudio.value.timestamp < frameEnd) {
          signal.throwIfAborted();
          const packet = nextAudio.value;
          if (packet.timestamp >= start && packet.timestamp < end) await audioSource.add(packet.clone({ timestamp: packet.timestamp - start }), { decoderConfig: audioConfig });
          nextAudio = await audioIterator.next();
        }
        frames++;
        if (frames % 5 === 0) {
          progress({ fraction: (frameEnd - start) / (end - start), frames });
          await new Promise(resolve => setTimeout(resolve, 0)); // Let cancel messages run.
        }
      } finally { sample.close(); }
    }
    if (frames === 0) throw new Error('No frames found in the selected range.');
    signal.throwIfAborted();
    await output.finalize();
    if (writable) await writable.close();
    progress({ fraction: 1, frames });
    return options.handle ? null : new Blob([target.buffer], { type: 'video/mp4' });
  } catch (error) {
    if (output && output.state !== 'finalized' && output.state !== 'canceled') await output.cancel().catch(() => {});
    if (writable) await writable.abort().catch(() => {});
    throw error;
  } finally { input.dispose(); }
}
