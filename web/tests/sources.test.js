import { test } from 'node:test';
import assert from 'node:assert/strict';
import { planActivities } from '../src/sources.js';
const a = {name:'phone.insgps'}, b = {name:'converted.gpx'}, other = {name:'readme.txt'};
const inspect = async file => ({start:'2026-07-29T14:30:05Z',end:'2026-07-29T14:35:00Z',matches:file!==b,videoUtc:'2026-07-29T14:30:04Z'});
test('multiple files choose only the compatible track and ignore unrelated files', async () => {
  const plan = await planActivities([a,b,other], inspect);
  assert.equal(plan.selected.file,a);
});
test('a folder with one incompatible file never falls back; single file may', async () => {
  assert.equal((await planActivities([b],inspect)).selected.file,b);
  const plan = await planActivities([b],inspect,true);
  assert.equal(plan.selected,undefined); assert.match(plan.message,/No activity matches/);
});
test('all matching candidates are offered, including different formats of the same track', async () => {
  const plan = await planActivities([a,b],async file => ({...await inspect(file),matches:true}));
  assert.deepEqual(plan.candidates.map(c=>c.file),[a,b]); assert.equal(plan.selected,undefined);
});
test('missing video time cannot silently select in batch, and invalid files are reported', async () => {
  const plan = await planActivities([a,b],async () => ({matches:false,videoUtc:null}));
  assert.match(plan.message,/Video time is unknown/);
  const broken = await planActivities([a],async () => {throw new Error('Invalid INSGPS records');},true,'2026-07-29T14:30:04Z');
  assert.match(broken.message,/No activity matches/); assert.match(broken.errors[0],/phone.insgps: Invalid/);
});
