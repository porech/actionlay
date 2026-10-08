import { openGpmf } from './telemetry.js';
import { exportVideo } from './export.js';
import { straightRgba } from './overlay.js';
let core;
let file;
let operation;
let reader;
let metadataOperation;
let duration;
let videoKey;
let queue = Promise.resolve();
self.onmessage = ({ data }) => {
  if (data.type === 'close') { metadataOperation?.abort(); reader = null; file = null; videoKey = null; return; }
  if (data.type === 'seek') { requestTelemetry(data.time, true); return; }
  if (data.type === 'cancel') { operation?.abort(); return; }
  // Rendering can sample the published telemetry while asynchronous file reads run.
  if (['render', 'layout', 'configure'].includes(data.type) && core) { run(data).catch(error => self.postMessage({id:data.id,error:String(error?.message ?? error)})); return; }
  queue = queue.then(() => run(data)).catch(error => self.postMessage({ id: data.id, error: String(error?.message ?? error) }));
};
async function run({ id, type, ...data }) {
  let result;
  if (type === 'init') {
    const module = await import(/* @vite-ignore */ `${data.base}pkg/actionlay_web.js`);
    await module.default();
    core = new module.Core();
  } else if (type === 'layout') {
    core.load_layout(data.bytes, 'layout.actionlay-layout');
  } else if (type === 'configure') {
    core.configure(data.units, data.maps, data.language);
  } else if (type === 'open') {
    videoKey = data.videoKey;
    file = data.file;
    core.reset_video(data.duration);
    metadataOperation = new AbortController();
    duration = data.duration;
    reader = await openGpmf(file, core, duration, metadataOperation.signal, utc => core.set_video_metadata_utc(utc ?? undefined));
    if (reader?.samples.length) reader.onPublish = () => self.postMessage({telemetryUpdated:true});
    else core.finish_telemetry(duration);
    result = { packets: reader?.samples.length ?? 0, videoUtc: core.video_utc() };
    requestTelemetry(0, true);
  } else if (['activity-context','activity-summary','activity-link','activity-offset','activity-unlink','video-utc'].includes(type)) {
    if (!file || videoKey !== data.videoKey) throw new Error('Open a video before linking an activity');
    if (type === 'activity-context') result = { videoUtc:core.video_utc() };
    else if (type === 'activity-summary' || type === 'activity-link') {
      if (data.file.size > 64 * 1024 * 1024) throw new Error('Activity file exceeds 64 MB');
      const bytes = new Uint8Array(await data.file.arrayBuffer());
      result = JSON.parse(type === 'activity-summary' ? core.activity_summary(data.file.name, bytes) : core.link_activity(data.file.name, bytes, data.offset));
    } else if (type === 'activity-offset') result = JSON.parse(core.set_activity_offset(data.offset));
    else if (type === 'video-utc') result = JSON.parse(core.set_video_utc(data.utc || undefined));
    else { core.unlink_activity(); result = {}; }
    self.postMessage({telemetryUpdated:true});
  } else if (type === 'render') {
    requestTelemetry(data.time);
    const pixels = straightRgba(core.render(data.time, data.width, data.height));
    self.postMessage({ id, result: {pixels, maps: JSON.parse(core.map_regions(data.width, data.height, data.time))} }, [pixels.buffer]);
    return;
  } else if (type === 'export') {
    if (!file) throw new Error('Open a video first.');
    operation = new AbortController();
    try {
      if (core.needs_full_history()) await reader?.readComplete(operation.signal);
      operation.signal.throwIfAborted();
      result = await exportVideo(file, core, data.options, operation.signal, progress => self.postMessage({ progress }), async time => {
        if (!reader || core.needs_full_history()) return;
        // Export advances monotonically; recover the required prefix/window before committing each frame.
        await reader.requestTime(time, false, operation.signal);
        await reader.requestHistory(JSON.parse(core.history_ranges(time, duration)), undefined, operation.signal);
        operation.signal.throwIfAborted();
      }); }
    finally { operation = null; }
  }
  self.postMessage({ id, result });
}

function requestTelemetry(time, seek = false) {
  if (!reader) return;
  const current = reader;
  const report = error => {
    if (error.name !== 'AbortError' && reader === current) self.postMessage({metadataError:String(error.message ?? error)});
  };
  current.requestTime(time, seek).catch(report);
  current.requestHistory(JSON.parse(core.history_ranges(time, duration)), (fraction,count,total,active) => {
    if (reader === current) self.postMessage({progress:{metadata:fraction,count,total,active}});
  }).catch(report);
}
