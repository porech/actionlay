import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createFile } from 'mp4box';
import { readGpmf, openGpmf } from '../src/telemetry.js';
// Two synthetic timed packets in a generic metadata track. No camera fixture needed.
function metadataFile(count = 2) {
  const mp4 = createFile();
  const id = mp4.addTrack({type:'mett',hdlr:'meta',timescale:1000,duration:count*1000,media_duration:count*1000});
  const entry = mp4.getTrackById(id).mdia.minf.stbl.stsd.entries[0];
  // GP metadata has the standard sample-entry header and no additional fields.
  entry.type = 'gpmd'; entry.data = new Uint8Array([0,0]);
  for(let i=0;i<count;i++) mp4.addSample(id, Uint8Array.from([1,2,3,4], x=>x+i*4), {dts:i*1000,cts:i*1000,duration:1000,is_sync:true});
  const buffer = Buffer.from(mp4.getBuffer().buffer);
  buffer.write('gpmd', buffer.indexOf('mett'), 'ascii');
  return new Blob([buffer]);
}
test('container extraction preserves every packet timestamp, duration and payload', async () => {
  const packets = [];
  const core = { add_packet(pts,duration,data) { packets.push({pts,duration,data:[...data]}); }, finish_telemetry() { return packets.length; } };
  const count = await readGpmf(metadataFile(), core, 2, new AbortController().signal);
  assert.equal(count, 2);
  assert.deepEqual(packets, [{pts:0,duration:1,data:[1,2,3,4]},{pts:1,duration:1,data:[5,6,7,8]}]);
});
test('metadata cancellation prevents publication', async () => {
  const controller = new AbortController(); controller.abort();
  await assert.rejects(readGpmf(metadataFile(), { add_packet() { assert.fail(); } }, 2, controller.signal), {name:'AbortError'});
});

test('playback reads only its window; seeks never trigger a full read', async () => {
  const order = [], publications = [];
  const reader = await openGpmf(metadataFile(10), {
    add_packet(pts) {order.push(pts);}, update_telemetry() {publications.push([...order]);},
    finish_telemetry() {assert.fail('partial playback completed the full track');},
  }, 10);
  await reader.requestTime(0);
  assert.deepEqual(order,[0]);
  await reader.requestTime(7.5,true);
  assert.deepEqual(order,[0,7,6,8]);
  assert.deepEqual(publications[0],[0]);
  assert.equal(reader.routeCursor,0);
});
test('independent route cursor continues in order through a foreground seek, sharing cached packets', async () => {
  const source = metadataFile(10), order = [], progress = [];
  const file = {size:source.size, slice(start,end) {const part=source.slice(start,end); return {async arrayBuffer() {
    if(part.size<=4) await new Promise(r=>setTimeout(r,10)); return part.arrayBuffer();
  }};}};
  let completed=0;
  const reader=await openGpmf(file, {
    add_packet(pts) {order.push(pts);}, update_telemetry() {}, finish_telemetry() {completed++;},
  },10);
  let foreground;
  await reader.requestRoute(10,(_,cursor,__,active) => {
    progress.push(cursor);
    if(cursor===1 && active) foreground=reader.requestTime(7.5,true);
  });
  await foreground;
  assert.equal(order.length,10); assert.equal(new Set(order).size,10);
  assert.equal(order[1],7);
  assert.equal(reader.routeCursor,10); assert.equal(completed,1);
  assert.deepEqual(progress,[...progress].sort((a,b)=>a-b));
});
test('cancelling a progressive read never publishes a complete track', async () => {
  const controller = new AbortController();
  await assert.rejects(readGpmf(metadataFile(10), {
    add_packet() {}, update_telemetry() {}, finish_telemetry() { assert.fail('cancelled track completed'); },
  }, 10, controller.signal, (_, n) => { if(n===1) controller.abort(); }), {name:'AbortError'});
});

test('removing the route requirement pauses background reads and keeps their cached progress', async () => {
  const order=[];
  const reader=await openGpmf(metadataFile(10), {
    add_packet(pts) {order.push(pts);}, update_telemetry() {}, finish_telemetry() {},
  },10);
  await reader.requestRoute(10,(_,cursor,__,active) => {if(cursor===1 && active) reader.requestRoute(0);});
  assert.deepEqual(order,[0]); assert.equal(reader.routeCursor,1);
  await reader.requestRoute(10);
  assert.equal(new Set(order).size,10); assert.equal(order.length,10);
});

test('history seeks read the requested window independently, without scanning the prefix or future', async () => {
  const order = [], validated = [];
  const reader = await openGpmf(metadataFile(100), {
    add_packet(pts) {order.push(pts);}, update_telemetry() {}, finish_telemetry() {assert.fail('window completed the source');},
    record_history_read(start,end) {validated.push([start,end]);},
  }, 100);
  await reader.requestTime(70,true);
  await reader.requestHistory([[60,70]]);
  assert.deepEqual(validated,[[60,70]]);
  assert(order.every(t => t>=60 && t<=70));
  await reader.requestTime(25,true);
  await reader.requestHistory([[15,25]]);
  assert.deepEqual(validated,[[60,70],[15,25]]);
  assert(order.every(t => (t>=60&&t<=70)||(t>=15&&t<=25)));
  assert.equal(order.length,new Set(order).size);
});

test('cancelling export history stops the read and a later preview can resume it', async () => {
  const controller=new AbortController(), order=[];
  const reader=await openGpmf(metadataFile(50), {
    add_packet(pts) {order.push(pts);}, update_telemetry() {}, finish_telemetry() {},
  },50);
  await assert.rejects(reader.requestHistory([[0,40]],(_,count,__,active) => {
    if(count===1 && active) controller.abort();
  },controller.signal),{name:'AbortError'});
  assert.equal(order.length,1);
  await reader.requestHistory([[0,40]]);
  assert.equal(order.length,40);
});
