import './style.css';
import { planActivities } from './sources.js';
import { initializeVideoDrop } from './drop.js';
import { setLanguage, t } from './i18n.js';
import { initializePlayer, refreshPlayer } from './player.js';
import { readState, writeState, rememberLayout, toBase64, fromBase64, defaultPreferences } from './storage.js';
const $ = id => document.getElementById(id);
const status = (message, error = false) => { $('status').removeAttribute('data-i18n'); $('status').textContent = t(message); $('status').classList.toggle('error', error); };
function requireSourceVideo() {
  if (loading || !$('video').src) {
    status('Open a video to link sources.', true);
    return false;
  }
  return true;
}
$('metric-sources').onclick = () => {
  if (!requireSourceVideo()) return;
  $('sources').open = true;
  $('sources').scrollIntoView({ block: 'nearest' });
};
$('sources').querySelector('summary').onclick = event => {
  if (!$('sources').open && !requireSourceVideo()) event.preventDefault();
};
const base = new URL(import.meta.env.BASE_URL, location.href).href;
const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
let requestId = 0;
const pending = new Map();
function rpc(type, data = {}) {
  const id = ++requestId;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    try { worker.postMessage({ id, type, ...data }); } catch (error) { pending.delete(id); reject(error); }
  });
}
worker.onmessage = ({ data }) => {
  if (data.telemetryUpdated) {
    if (data.videoKey === sourceGeneration && data.cameraMetrics !== undefined) $('camera-metrics').hidden = !data.cameraMetrics;
    lastTime = -1; return;
  }
  if (data.metadataError) { status(data.metadataError, true); return; }
  if (data.progress) {
    if (data.progress.metadata !== undefined) {
      updateMapLoaders();
      lastTime = -1;
    }
    else { $('progress').value = data.progress.fraction; status(t('Exporting… {percent}% · {frames} frames', {percent:Math.round(data.progress.fraction * 100),frames:data.progress.frames})); }
    return;
  }
  const request = pending.get(data.id);
  if (request) { pending.delete(data.id); data.error ? request.reject(new Error(data.error)) : request.resolve(data.result); }
};
worker.onerror = event => { for (const p of pending.values()) p.reject(new Error(event.message || t('Media worker failed.'))); pending.clear(); status('Media worker failed. Reload ActionLay to restart it.', true); };
let state;
let storageAvailable = true;
try { state = readState(localStorage); } catch (error) { state = { preferences: defaultPreferences(), layouts: [] }; storageAvailable = false; status(String(error.message), true); }
let core;
let module;
let currentId = state.preferences.selected;
let currentName = 'Layout';
let videoUrl;
let generation = 0;
let sourceGeneration = 0;
let activityGeneration = 0;
let activityFile;
let activityInfo;
let activityNotice;
let videoIdentity;
let activitySettings = {};
const FALLBACK = 'Timestamps could not be matched. Starts were aligned. Adjust the activity offset if needed.';
function activityStatus(message, parameters = {}, error = false) {
  activityNotice = {message,parameters,error};
  const element = $('activity-status'); element.textContent = t(message, parameters); element.classList.toggle('error', error);
}
function showActivityInfo(info) {
  activityInfo = info;
  activityStatus(info.fallback ? FALLBACK : '{} activity samples linked', {samples:info.samples});
}
function saveActivitySettings() {
  if (!videoIdentity) return;
  activitySettings = { utc:$('video-utc').value, offset:Number($('activity-offset').value), activity:activityFile ? `${activityFile.name}|${activityFile.size}|${activityFile.lastModified}` : null };
  const sources = {...state.preferences.videoSources, [videoIdentity]:activitySettings};
  // Keep the browser's small per-video settings bounded; file contents are never stored.
  const entries = Object.entries(sources).slice(-100);
  try { persist({...state, preferences:{...state.preferences, videoSources:Object.fromEntries(entries)}}); }
  catch (error) { status(error.message, true); }
}
async function linkActivity(file) {
  const mine = sourceGeneration, request = ++activityGeneration;
  const identity = `${file.name}|${file.size}|${file.lastModified}`;
  const offset = activitySettings.activity === identity ? activitySettings.offset ?? 0 : 0;
  const info = await rpc('activity-link', {file, offset, videoKey:mine});
  if (mine !== sourceGeneration || request !== activityGeneration) return;
  activityFile = file; $('activity-offset').value = String(offset);
  $('activity-candidates').replaceChildren();
  showActivityInfo(info); saveActivitySettings(); lastTime = -1;
}
async function selectActivities(files, strict = false) {
  if (loading || !$('video').src) throw new Error('Open a video before linking an activity');
  const mine = sourceGeneration, request = ++activityGeneration;
  $('activity-candidates').replaceChildren(); activityStatus('Reading activity…');
  const {videoUtc} = await rpc('activity-context', {videoKey:mine});
  const plan = await planActivities(files, async file => { try { return await rpc('activity-summary', {file, videoKey:mine}); } catch (error) { throw new Error(t(error.message)); } }, strict, videoUtc);
  if (mine !== sourceGeneration || request !== activityGeneration) return;
  if (plan.selected) { await linkActivity(plan.selected.file); return; }
  activityInfo = null;
  activityStatus(plan.message, {}, true);
  for (const candidate of plan.candidates) {
    const button = document.createElement('button'); button.type = 'button';
    button.textContent = `${candidate.file.webkitRelativePath || candidate.file.name} · ${candidate.start} — ${candidate.end}`;
    button.onclick = guarded(() => linkActivity(candidate.file));
    $('activity-candidates').append(button);
  }
  for (const error of plan.errors) {
    const element = document.createElement('p'); element.textContent = `${t('Activity file could not be read')}: ${error}`;
    $('activity-candidates').append(element);
  }
}
let layoutRevision = 0;
let loading = false;
let mapRegions = [];
let exporting = false;
let editorStarted = false;
let editorOrigin;
let rendering = false;
let lastRender = 0;
let lastTime = -1;
let mediaTime;
function persist(next) {
  writeState(localStorage, next);
  state = next;
}
function guarded(fn) { return (...args) => Promise.resolve().then(() => fn(...args)).catch(error => status(error.message ?? String(error), true)); }
function download(blob, name) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url; anchor.download = name; anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 30_000);
}
function packageName() { return `${currentName.replace(/[^\p{L}\p{N} _.-]/gu, '_').replace(/\.(actionlay-layout|ovl\.json|xml)$/i, '') || 'Layout'}.actionlay-layout`; }
function updateLayouts() {
  const select = $('layouts');
  select.replaceChildren();
  const bundled = document.createElement('optgroup'); bundled.label = t('Included layouts');
  for (const preset of JSON.parse(module.Core.presets())) bundled.append(new Option(preset.name, `builtin:${preset.id}`));
  const recent = document.createElement('optgroup'); recent.label = t('Recent layouts');
  for (const entry of state.layouts) recent.append(new Option(entry.name, `recent:${entry.id}`));
  select.append(bundled, recent);
  select.value = currentId && state.layouts.some(x => x.id === currentId) ? `recent:${currentId}` : `builtin:${state.preferences.builtin ?? 'default'}`;
  $('forget').disabled = !currentId || !state.layouts.some(x => x.id === currentId);
}
async function syncLayout() {
  layoutRevision++;
  lastTime = -1;
  await rpc('layout', { bytes: core.package_bytes() });
}
async function configure() {
  const prefs = state.preferences;
  const language = await setLanguage(prefs.language);
  core.configure(prefs.units, prefs.maps, language);
  await rpc('configure', {...prefs, language});
  updateExportControls();
  refreshPlayer();
  updateLayouts();
  if (activityNotice) activityStatus(activityNotice.message, activityNotice.parameters, activityNotice.error);
  layoutRevision++;
  lastTime = -1;
}
async function rememberCurrent() {
  const bytes = core.package_bytes();
  const id = currentId ?? crypto.randomUUID();
  const name = JSON.parse(core.layout_json()).name || currentName;
  const next = rememberLayout(state, { id, name, bytes: toBase64(bytes) });
  persist(next);
  currentId = id;
  currentName = name;
  core.acknowledge_save();
  if (editorOpen()) editorOrigin = { id: currentId, name: currentName };
  await syncLayout();
  updateLayouts();
}
function editorOpen() { return !$('editor').hidden; }
function confirmDiscard() { return !editorOpen() || !core.editor_dirty() || confirm(t('Discard unsaved layout changes?')); }
async function openEditor(isNew = false) {
  editorOrigin = { id: currentId, name: currentName };
  if (isNew) { core.new_layout(); currentId = null; currentName = 'Untitled'; }
  else core.start_editing();
  $('editor').hidden = false;
  if (!editorStarted) { await module.start_editor(core, $('editor-canvas')); editorStarted = true; }
}
function closeEditor() { if (!confirmDiscard()) return; core.close_editor(); $('editor').hidden = true; if (editorOrigin) { currentId = editorOrigin.id; currentName = editorOrigin.name; } updateLayouts(); }
function setBusy(busy) {
  for (const element of document.querySelectorAll('.toolbar button,.toolbar input,.toolbar select,.settings input,.settings select,#edit,#new,#export')) element.disabled = busy;
  $('export').disabled = busy || !$('video').src;
  if (!busy) updateLayouts();
}
async function openVideo(file) {
  if (!file) return;
  const mine = ++sourceGeneration;
  activityGeneration++; activityFile = null; activityInfo = null; activityNotice = null;
  $('camera-metrics').hidden = true;
  videoIdentity = `${file.name}|${file.size}|${file.lastModified}`;
  activitySettings = state.preferences.videoSources?.[videoIdentity] ?? {};
  $('activity-offset').value = String(activitySettings.offset ?? 0);
  $('video-utc').value = activitySettings.utc ?? '';
  $('activity-candidates').replaceChildren(); $('activity-status').textContent = '';
  generation++;
  loading = true; lastTime = -1; mediaTime = undefined;
  worker.postMessage({ type: 'close' });
  const video = $('video');
  video.pause();
  if (videoUrl) URL.revokeObjectURL(videoUrl);
  videoUrl = URL.createObjectURL(file);
  $('overlay').getContext('2d').clearRect(0, 0, $('overlay').width, $('overlay').height);
  $('empty').hidden = true;
  $('export').disabled = true;
  mapRegions = []; updateMapLoaders();
  status(t('Opening {name}…', {name:file.name}));
  const metadata = new Promise((resolve, reject) => {
    const cleanup = () => { video.removeEventListener('loadedmetadata', loaded); video.removeEventListener('error', failed); };
    const loaded = () => { cleanup(); resolve(); };
    const failed = () => { cleanup(); reject(new Error('This browser cannot play the video’s container or codec. Try a supported MP4 or the desktop app.')); };
    video.addEventListener('loadedmetadata', loaded); video.addEventListener('error', failed);
  });
  video.src = videoUrl;
  try {
    await metadata;
    if (mine !== sourceGeneration) return;
    if (!Number.isFinite(video.duration) || video.duration <= 0) throw new Error('Video duration is unavailable.');
    $('start').value = '0'; $('end').value = String(video.duration);
    $('player-controls').hidden = false;
    refreshPlayer();
    const info = await rpc('open', { file, duration: video.duration, videoKey:mine });
    if (mine === sourceGeneration) $('camera-metrics').hidden = !info.cameraMetrics;
    if (activitySettings.utc) await rpc('video-utc', {utc:activitySettings.utc,videoKey:mine});
    if (mine !== sourceGeneration) return;
    status(file.name);
    $('export').disabled = false;
  } catch (error) { if (mine === sourceGeneration) status(error.message, true); }
  finally { if (mine === sourceGeneration) { loading = false; updateMapLoaders(); } }
}
async function preview(now) {
  requestAnimationFrame(preview);
  const video = $('video');
  if (!core || !video.src || !video.videoWidth || exporting || rendering || editorOpen() || now - lastRender < 40) return;
  const time = video.paused || video.seeking ? video.currentTime : (mediaTime ?? video.currentTime);
  // Redraw while paused too: asynchronous map tiles may have arrived.
  if (time === lastTime && now - lastRender < 500) return;
  const bounds = video.getBoundingClientRect();
  const ratio = Math.min(bounds.width / video.videoWidth, bounds.height / video.videoHeight);
  const displayWidth = video.videoWidth * ratio;
  const displayHeight = video.videoHeight * ratio;
  const width = Math.max(1, Math.round(Math.min(displayWidth * devicePixelRatio, 1280)));
  const height = Math.max(1, Math.round(width * video.videoHeight / video.videoWidth));
  const canvas = $('overlay');
  canvas.style.width = `${displayWidth}px`; canvas.style.height = `${displayHeight}px`;
  rendering = true; lastRender = now;
  const mine = generation, revision = layoutRevision;
  try {
    const {pixels, maps} = await rpc('render', { time, width, height });
    if (mine !== generation || revision !== layoutRevision || video.seeking) return;
    if (canvas.width !== width || canvas.height !== height) { canvas.width = width; canvas.height = height; }
    canvas.getContext('2d').putImageData(new ImageData(pixels, width, height), 0, 0);
    mapRegions = maps;
    const loaders = $('map-loaders');
    loaders.style.width = `${displayWidth}px`; loaders.style.height = `${displayHeight}px`;
    updateMapLoaders();
    lastTime = time;
  } catch (error) { status(t('Overlay: {error}', {error:t(error.message)}), true); }
  finally { rendering = false; }
}
function trackVideoFrames() {
  if ($('video').requestVideoFrameCallback) $('video').requestVideoFrameCallback((_, metadata) => { mediaTime = metadata.mediaTime; trackVideoFrames(); });
}
async function exportToDevice() {
  if (loading || exporting || !$('video').src) return;
  const options = { mode: $('mode').value, codec: $('codec').value, color: $('color').value, start: Number($('start').value), end: Number($('end').value) };
  // Invoke before any await so the native save picker retains user activation.
  const pick = window.showSaveFilePicker ? window.showSaveFilePicker({ suggestedName: 'actionlay.mp4', types: [{ description: t('MP4 video'), accept: { 'video/mp4': ['.mp4'] } }] }) : null;
  try {
    if (pick) options.handle = await pick;
  } catch (error) { if (error.name === 'AbortError') return; throw error; }
  exporting = true;
  $('video').pause();
  setBusy(true); $('cancel').hidden = false; $('progress').hidden = false; $('progress').value = 0;
  try {
    const blob = await rpc('export', { options });
    if (blob) download(blob, 'actionlay.mp4');
    status('Export complete.');
  } catch (error) { status(error.message.includes('abort') ? 'Export cancelled.' : t('Export failed: {error}', {error:t(error.message)}), !error.message.includes('abort')); }
  finally { exporting = false; setBusy(false); $('cancel').hidden = true; $('progress').hidden = true; }
}
async function initialize() {
  setBusy(true);
  await setLanguage(state.preferences.language);
  module = await import(/* @vite-ignore */ `${base}pkg/actionlay_web.js`);
  await module.default();
  core = new module.Core();
  await rpc('init', { base });
  const previous = state.layouts.find(x => x.id === currentId);
  let restoreWarning;
  try {
    if (previous) { core.load_layout(fromBase64(previous.bytes), 'layout.actionlay-layout'); currentName = previous.name; }
    else { currentId = null; core.select_preset(state.preferences.builtin ?? 'default'); }
  } catch (error) { currentId = null; restoreWarning = t('The last layout could not be restored: {error}. The default is open; saved recents are preserved.', {error:t(error.message)}); }
  updateLayouts();
  for (const id of ['units', 'language', 'mode', 'codec']) $(id).value = state.preferences[id];
  $('maps').checked = state.preferences.maps;
  await configure(); await syncLayout();
  setBusy(false);
  if (restoreWarning) status(restoreWarning, true);
  else if (storageAvailable) status('Ready. Preferences and recent layouts are saved in this browser.');
  trackVideoFrames(); requestAnimationFrame(preview);
}
initializeVideoDrop({
  open: guarded(openVideo),
  canOpen: () => Boolean(core) && !$('video-file').disabled && !exporting,
  hint: $('drop-hint'),
  reject: () => status('Drop a video file to open it.', true),
});
$('video-file').onchange = guarded(event => openVideo(event.target.files[0]));
$('activity-file').onchange = guarded(async event => { const files = [...event.target.files]; event.target.value = ''; if (files.length) await selectActivities(files); });
$('activity-folder').onchange = guarded(async event => { const files = [...event.target.files]; event.target.value = ''; if (files.length) await selectActivities(files, true); });
$('activity-offset').onchange = guarded(async () => {
  if (loading || !$('video').src) throw new Error('Open a video before linking an activity');
  const mine = sourceGeneration;
  const info = await rpc('activity-offset', {offset:$('activity-offset').valueAsNumber,videoKey:mine});
  if (mine !== sourceGeneration) return;
  if (activityFile) showActivityInfo(info);
  saveActivitySettings(); lastTime = -1;
});
$('video-utc').onchange = guarded(async () => {
  if (loading || !$('video').src) throw new Error('Open a video before linking an activity');
  const mine = sourceGeneration;
  const info = await rpc('video-utc', {utc:$('video-utc').value,videoKey:mine});
  if (mine !== sourceGeneration) return;
  if (activityFile) showActivityInfo(info);
  saveActivitySettings(); lastTime = -1;
});
$('activity-unlink').onclick = guarded(async () => {
  if (loading || !$('video').src) throw new Error('Open a video before linking an activity');
  activityGeneration++;
  await rpc('activity-unlink', {videoKey:sourceGeneration});
  activityFile = null; activityInfo = null; activityNotice = null; $('activity-offset').value = '0';
  $('activity-status').textContent = ''; $('activity-candidates').replaceChildren(); saveActivitySettings(); lastTime = -1;
});
$('layout-file').onchange = guarded(async event => {
  const file = event.target.files[0]; if (!file || !confirmDiscard()) return;
  if (file.size > 128 * 1024 * 1024) throw new Error('Layout package exceeds 128 MB.');
  const warning = core.load_layout(new Uint8Array(await file.arrayBuffer()), file.name);
  $('editor').hidden = true; currentId = null; currentName = file.name;
  await syncLayout();
  try { await rememberCurrent(); status(`${t('Layout imported and saved in recents.')}${warning ? '\n' + warning : ''}`); }
  catch (error) { status(t('Layout imported. {error}', {error:t(error.message)}), true); }
  event.target.value = '';
});
$('layouts').onchange = guarded(async event => {
  if (!confirmDiscard()) { updateLayouts(); return; }
  const [kind, id] = event.target.value.split(':');
  if (kind === 'builtin') {
    core.select_preset(id); currentId = null; currentName = JSON.parse(core.layout_json()).name ?? id;
    const next = { ...state, preferences: { ...state.preferences, builtin: id, selected: null } };
    try { persist(next); } catch (error) { status(error.message, true); }
  } else {
    const entry = state.layouts.find(x => x.id === id);
    core.load_layout(fromBase64(entry.bytes), 'layout.actionlay-layout'); currentId = id; currentName = entry.name;
    try { persist(rememberLayout(state, entry)); } catch (error) { status(error.message, true); }
  }
  $('editor').hidden = true; await syncLayout(); updateLayouts();
});
$('edit').onclick = guarded(() => openEditor());
$('new').onclick = guarded(async () => { if (!confirmDiscard()) return; await openEditor(true); });
$('save').onclick = guarded(() => { download(new Blob([core.package_bytes()], { type: 'application/zip' }), packageName()); return rememberCurrent().catch(error => status(t('Layout downloaded. {error}', {error:t(error.message)}), true)); });
$('forget').onclick = guarded(() => { if (!currentId) return; persist({ ...state, layouts: state.layouts.filter(x => x.id !== currentId), preferences: { ...state.preferences, selected: null } }); currentId = null; updateLayouts(); status('Removed from recents. The current layout remains open.'); });
for (const id of ['units','maps','language','codec','mode']) $(id).onchange = guarded(async () => {
  const value = id === 'maps' ? $(id).checked : $(id).value;
  const next = { ...state, preferences: { ...state.preferences, [id]: value, ...(id === 'language' ? {languageChosen:true} : {}) } };
  try { persist(next); } catch (error) { state = next; status(t('Preference applied for this session. {error}', {error:t(error.message)}), true); }
  await configure();
});
$('asset-file').onchange = guarded(async event => { const file = event.target.files[0]; if (!file) return; if (file.size > 32 * 1024 * 1024) throw new Error('Asset exceeds 32 MB.'); core.attach_asset(file.name, new Uint8Array(await file.arrayBuffer())); event.target.value = ''; });
$('editor-save').onclick = guarded(async () => { await rememberCurrent(); status('Layout saved and applied.'); });
$('editor-download').onclick = guarded(() => { download(new Blob([core.package_bytes()], { type: 'application/zip' }), packageName()); });
$('editor-close').onclick = closeEditor;
$('export').onclick = () => exportToDevice().catch(error => status(error.message, true));
$('cancel').onclick = () => worker.postMessage({ type: 'cancel' });
$('video').addEventListener('seeking', () => { generation++; mediaTime = undefined; lastTime = -1; worker.postMessage({type:'seek',time:$('video').currentTime}); });
window.addEventListener('beforeunload', event => { if (exporting || (core && core.editor_dirty())) { event.preventDefault(); event.returnValue = ''; } });
setInterval(guarded(async () => {
  if (!core || !editorOpen()) return;
  const action = core.take_editor_action();
  if (action === 1) { await rememberCurrent(); status('Layout saved and applied.'); }
  else if (action === 2) closeEditor();
  else if (action === 3 && confirmDiscard()) { core.new_layout(); currentId = null; currentName = 'Untitled'; }
}), 100);
function updateMapLoaders() {
  const container = $('map-loaders');
  container.hidden = !mapRegions.length;
  // Reuse animated elements across preview frames so their rotation is continuous.
  if (container.children.length !== mapRegions.length) {
    container.replaceChildren(...mapRegions.map(() => {
      const loader = document.createElement('div'); loader.className = 'map-loader';
      loader.setAttribute('role', 'status');
      loader.innerHTML = '<span class="map-spinner"></span><span class="map-percent"></span>';
      return loader;
    }));
  }
  mapRegions.forEach(([x,y,w,h,fraction], i) => {
    const loader = container.children[i];
    loader.style.left = `${(x+w/2)*100}%`; loader.style.top = `${(y+h/2)*100}%`;
    loader.setAttribute('aria-label', t('Reading telemetry…'));
    loader.lastElementChild.textContent = `${Math.floor(fraction*100)}%`;
  });
}
function updateExportControls() { $('color-control').hidden = $('mode').value !== 'solid'; }
initializePlayer(error => status(error.message, true));
initialize().catch(error => status(t('Unable to start ActionLay: {error}', {error:t(error.message)}), true));
