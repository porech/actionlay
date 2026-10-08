import { test } from 'node:test';
import assert from 'node:assert/strict';
import { straightRgba } from '../src/overlay.js';
test('canvas conversion preserves transparency and prevents dark halos', () => {
  assert.deepEqual(straightRgba(new Uint8Array([64,32,0,128,0,0,0,0,255,255,255,255])), new Uint8ClampedArray([128,64,0,128,0,0,0,0,255,255,255,255]));
});
