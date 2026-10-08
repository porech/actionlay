import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readState, writeState, rememberLayout, toBase64, fromBase64, STORAGE_KEY } from '../src/storage.js';
const store = () => { const data = new Map(); return { getItem: key => data.get(key) ?? null, setItem: (key, value) => data.set(key, value) }; };
test('preferences and complete layout packages survive restart, recents remain unique and limited', () => {
  const storage = store(); let state = readState(storage);
  state.preferences.units = 'imperial';
  const bytes = Uint8Array.from({ length: 20000 }, (_, i) => i % 256);
  for (let i = 0; i < 12; i++) state = rememberLayout(state, { id: String(i), name: `Layout ${i}`, bytes: toBase64(bytes) });
  state = rememberLayout(state, state.layouts[4]); writeState(storage, state);
  const restored = readState(storage);
  assert.equal(restored.preferences.units, 'imperial'); assert.equal(restored.layouts.length, 10);
  assert.equal(new Set(restored.layouts.map(x => x.id)).size, 10);
  assert.equal(restored.layouts[0].id, restored.preferences.selected);
  assert.deepEqual(fromBase64(restored.layouts[0].bytes), bytes);
});
test('quota failures and oversized packages preserve the last successful state', () => {
  const storage = store(); const state = readState(storage); writeState(storage, state);
  const original = storage.getItem(STORAGE_KEY);
  assert.throws(() => writeState(storage, rememberLayout(state, { id: 'big', name: 'Big', bytes: 'x'.repeat(4 * 1024 * 1024) })));
  assert.equal(storage.getItem(STORAGE_KEY), original);
  assert.throws(() => writeState({ setItem() { throw new Error('QuotaExceededError'); } }, state));
});
test('corrupt storage is reported instead of silently replacing saved layouts', () => {
  const storage = store(); storage.setItem(STORAGE_KEY, '{'); assert.throws(() => readState(storage));
});

test('browser language migration retains layouts and explicit preferences', () => {
  const storage = store();
  const layouts = [{id:'ride',name:'Ride',bytes:'AQID'}];
  storage.setItem(STORAGE_KEY, JSON.stringify({version:1,preferences:{language:'en'},layouts}));
  assert.equal(readState(storage).preferences.language, 'system');
  assert.deepEqual(readState(storage).layouts, layouts);
  storage.setItem(STORAGE_KEY, JSON.stringify({version:1,preferences:{language:'en',languageChosen:true},layouts}));
  assert.equal(readState(storage).preferences.language, 'en');
});
