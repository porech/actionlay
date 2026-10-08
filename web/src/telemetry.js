import { createFile } from 'mp4box';
// First read only the container index. Then read just the GPMF samples by offset.
// The source video is never copied wholesale into JS or WASM memory.
export async function openGpmf(file, core, duration, signal, onMetadata = () => {}) {
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
  const date = [movie.videoTracks?.[0]?.created, movie.created]
    .filter(value => value != null).map(value => value instanceof Date ? value : new Date(value))
    .find(value => Number.isFinite(value.getTime()) && value.getUTCFullYear() >= 1970);
  onMetadata(date?.toISOString() ?? null);
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
  requestTime(time, seek = false, signal) {
    if (signal) this.foregroundSignal = signal;
    if (!this.samples.length || this.finished) return Promise.resolve();
    if (!seek && this.lastTime !== undefined && Math.abs(time-this.lastTime)<.25) return this.foreground ?? Promise.resolve();
    this.lastTime = time; this.target = time; this.epoch++;
    if (!this.foreground) {
      this.foreground = this.readForeground().catch(error => { this.lastTime=undefined; throw error; }).finally(() => { this.foreground = null; this.foregroundSignal=undefined; });
    }
    return this.foreground;
  }
  async readForeground() {
    while (this.target !== undefined) {
      const time = this.target, epoch = this.epoch; this.target = undefined;
      const current = this.indexAt(time), first = this.indexAt(Math.max(0,time-1));
      const last = this.indexAt(Math.min(this.duration,time+.5));
      // Publish the sought frame first, then its filter/interpolation context.
      const signal = this.foregroundSignal;
      await this.readSample(current, signal); this.publish(true);
      for (let i=first; i<=last; i++) {
        if (epoch !== this.epoch) break;
        await this.readSample(i, signal); await yieldTask();
      }
      this.publish(true);
    }
  }
  requestRoute(end, progress = () => {}) {
    return this.requestHistory(end > 0 ? [[0,end]] : [], progress);
  }
  requestHistory(ranges, progress = () => {}, signal) {
    if (signal) this.historySignal = signal;
    const key = JSON.stringify(ranges.map(([a,b]) => [this.indexAt(a),this.indexAt(Math.max(a,b-1e-6))]));
    if (key === this.historyKey) return this.route ?? Promise.resolve();
    this.historyKey = key; this.historyEpoch = (this.historyEpoch ?? 0) + 1;
    this.historyRanges = ranges;
    this.routeEnd = Math.max(0, ...ranges.map(r => r[1]));
    if (!ranges.length || !this.samples.length) return this.route ?? Promise.resolve();
    if (!this.route) this.route = this.readRoute(progress).catch(error => { this.historyKey=undefined; throw error; }).finally(() => { this.route = null; this.historySignal=undefined; });
    return this.route;
  }
  async readRoute(progress) {
    let lastProgress = 0;
    try {
      let epoch;
      do {
        epoch = this.historyEpoch;
        const ranges = this.historyRanges;
        const indices = ranges.map(([start,end]) => [this.indexAt(start), this.indexAt(Math.max(start,end-1e-6))]);
        const total = indices.reduce((sum,[a,b]) => sum+b-a+1, 0);
        let count = 0;
        for (let r=0; r<ranges.length && epoch===this.historyEpoch; r++) {
          const [first,last] = indices[r];
          for (let i=first; i<=last && epoch===this.historyEpoch; i++) {
            // Separate from playback's cursor; shared cache deduplicates the reads.
            this.check(this.historySignal);
            if (first===0) this.routeCursor = Math.max(this.routeCursor,i+1);
            if (this.loaded.has(i)) { count++; continue; }
            await this.readSample(i, this.historySignal); count++;
            if (performance.now()-lastProgress>=150 || count===1) {
              progress(count/total, count, total, true); lastProgress=performance.now();
            }
            await yieldTask();
          }
          if (epoch===this.historyEpoch) {
            const next = this.samples[last+1];
            const validatedEnd = Math.max(ranges[r][1], next ? next.cts/next.timescale : this.duration);
            this.core.record_history_read?.(ranges[r][0], validatedEnd);
            this.onPublish?.();
          }
        }
      } while (epoch!==this.historyEpoch && this.historyRanges.length);
      this.publish(true);
    } finally { progress(1, this.loaded.size, this.samples.length, false); }
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
