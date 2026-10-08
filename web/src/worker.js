import { openGpmf } from './telemetry.js';
import { exportVideo } from './export.js';
import { straightRgba } from './overlay.js';
let core;
let file;
let operation;
let reader;
let metadataOperation;
let duration;
let queue = Promise.resolve();
self.onmessage = ({ data }) => {
  if (data.type === 'close') { metadataOperation?.abort(); reader = null; return; }
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
    file = data.file;
    core.reset_video(data.duration);
    metadataOperation = new AbortController();
    duration = data.duration;
    reader = await openGpmf(file, core, duration, metadataOperation.signal);
    if (reader) reader.onPublish = () => self.postMessage({telemetryUpdated:true});
    result = reader?.samples.length ?? 0;
    requestTelemetry(0, true);
  } else if (type === 'render') {
    requestTelemetry(data.time);
    const pixels = straightRgba(core.render(data.time, data.width, data.height));
    self.postMessage({ id, result: {pixels, maps: JSON.parse(core.map_regions(data.width, data.height))} }, [pixels.buffer]);
    return;
  } else if (type === 'export') {
    if (!file) throw new Error('Open a video first.');
    operation = new AbortController();
    try {
      await reader?.readComplete(operation.signal);
      operation.signal.throwIfAborted();
      result = await exportVideo(file, core, data.options, operation.signal, progress => self.postMessage({ progress })); }
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
  current.requestRoute(core.route_read_until(time, duration), (fraction,count,total,active) => {
    if (reader === current) self.postMessage({progress:{metadata:fraction,count,total,active}});
  }).catch(report);
}
