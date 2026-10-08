import { createFile } from 'mp4box';
// First read only the container index. Then read just the GPMF samples by offset.
// The source video is never copied wholesale into JS or WASM memory.
export async function openGpmf(file, core, duration, signal) {
  const mp4 = createFile(false);
  let movie;
  let failure;
  mp4.onReady = info => { movie = info; };
  mp4.onError = message => { failure = new Error(`MP4 metadata: ${message}`); };
  let position = 0;
  while ((!movie || movie.isFragmented) && position < file.size) {
    signal?.throwIfAborted();
    const buffer = await file.slice(position, Math.min(file.size, position + 1024 * 1024)).arrayBuffer();
    buffer.fileStart = position;
    const next = mp4.appendBuffer(buffer);
    if (failure) throw failure;
    position = next > position ? next : position + buffer.byteLength;
  }
  mp4.flush();
  if (!movie) return null; // Non-MP4 playback can still work, without GoPro telemetry.
  const track = movie.tracks.find(t => t.codec === 'gpmd');
  if (!track) return null;
  const samples = mp4.getTrackById(track.id).samples;
  return new TelemetryReader(file, samples, core, duration, signal);
}

const yieldTask = () => new Promise(resolve => setTimeout(resolve, 0));
export class TelemetryReader {
  constructor(file, samples, core, duration, signal) {
    Object.assign(this, {file, samples, core, duration, signal});
    this.loaded = new Set(); this.inflight = new Map(); this.finished = false;
    this.lastPublish = 0; this.epoch = 0; this.routeCursor = 0; this.routeEnd = 0;
  }
  check(signal) { this.signal?.throwIfAborted(); signal?.throwIfAborted(); }
  publish(force = false) {
    if (this.finished || !this.loaded.size) return;
    if (this.loaded.size === this.samples.length) {
      this.core.finish_telemetry(this.duration); this.finished = true; this.onPublish?.();
    } else if (force || performance.now() - this.lastPublish >= 150) {
      this.core.update_telemetry?.(); this.lastPublish = performance.now(); this.onPublish?.();
    }
  }
  async readSample(index, signal) {
    this.check(signal);
    if (this.loaded.has(index)) return;
    if (!this.inflight.has(index)) {
      const sample = this.samples[index];
      this.inflight.set(index, (async () => {
        if (sample.size > 16 * 1024 * 1024) throw new Error('GPMF packet exceeds 16 MB.');
        const bytes = new Uint8Array(await this.file.slice(sample.offset, sample.offset + sample.size).arrayBuffer());
        this.check();
        this.core.add_packet(sample.cts / sample.timescale, sample.duration / sample.timescale, bytes);
        this.loaded.add(index); this.publish(this.loaded.size === 1);
      })());
    }
    try { await this.inflight.get(index); this.check(signal); }
    finally { this.inflight.delete(index); }
  }
  indexAt(time) {
    let low = 0, high = this.samples.length;
    while (low < high) {
      const middle = (low + high) >>> 1, sample = this.samples[middle];
      if ((sample.cts + sample.duration) / sample.timescale <= time) low = middle + 1;
      else high = middle;
    }
    return Math.min(low, this.samples.length - 1);
  }
  requestTime(time, seek = false) {
    if (!this.samples.length || this.finished) return Promise.resolve();
    if (!seek && this.lastTime !== undefined && Math.abs(time-this.lastTime)<.25) return this.foreground ?? Promise.resolve();
    this.lastTime = time; this.target = time; this.epoch++;
    if (!this.foreground) {
      this.foreground = this.readForeground().finally(() => { this.foreground = null; });
    }
    return this.foreground;
  }
  async readForeground() {
    while (this.target !== undefined) {
      const time = this.target, epoch = this.epoch; this.target = undefined;
      const current = this.indexAt(time), first = this.indexAt(Math.max(0,time-1));
      const last = this.indexAt(Math.min(this.duration,time+.5));
      // Publish the sought frame first, then its filter/interpolation context.
      await this.readSample(current); this.publish(true);
      for (let i=first; i<=last; i++) {
        if (epoch !== this.epoch) break;
        await this.readSample(i); await yieldTask();
      }
      this.publish(true);
    }
  }
  requestRoute(end, progress = () => {}) {
    // Removing route maps pauses their reader, retaining its cursor and cache.
    if (end <= 0) { this.routeEnd = 0; return this.route ?? Promise.resolve(); }
    if (!this.samples.length || end <= this.routeEnd) return this.route ?? Promise.resolve();
    this.routeEnd = end;
    if (!this.route) this.route = this.readRoute(progress).finally(() => { this.route = null; });
    return this.route;
  }
  async readRoute(progress) {
    let lastProgress = 0;
    try {
      while (this.routeCursor < this.samples.length) {
        const sample = this.samples[this.routeCursor];
        if (this.routeEnd < this.duration && sample.cts / sample.timescale >= this.routeEnd) break;
        // This cursor only moves forward. Playback seeks never modify it.
        await this.readSample(this.routeCursor++);
        const total = this.routeEnd >= this.duration ? this.samples.length : this.indexAt(this.routeEnd)+1;
        if (performance.now()-lastProgress>=150 || this.routeCursor===1) {
          progress(this.routeCursor/total, this.routeCursor, total, true); lastProgress=performance.now();
        }
        await yieldTask();
      }
      this.publish(true);
    } finally { progress(1, this.routeCursor, this.samples.length, false); }
  }
  async readComplete(signal, progress = () => {}) {
    for(let i=0;i<this.samples.length;i++) {
      this.check(signal); await this.readSample(i, signal);
      progress((i+1)/this.samples.length, i+1, this.samples.length);
      await yieldTask();
    }
    this.publish(true); return this.loaded.size;
  }
}

// Complete extraction for callers that explicitly require a validated full track.
export async function readGpmf(file, core, duration, signal, progress = () => {}) {
  const reader = await openGpmf(file, core, duration, signal);
  return reader ? reader.readComplete(signal, progress) : 0;
}
