import { test, expect } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
const directory = resolve('test-results/activities');
const video = resolve('test-results/dated-video.mp4');
const start = Date.parse('2026-07-29T14:30:04Z')/1000;
function insgps(seconds) {
  const buffer = Buffer.alloc(53*4);
  for (let i=0;i<4;i++) {
    const n=i*53; buffer.writeBigUInt64LE(BigInt(seconds+i),n); buffer[n+10]=65;
    buffer.writeDoubleLE(45,n+11); buffer[n+19]=78;
    buffer.writeDoubleLE(7,n+20); buffer[n+28]=69;
    buffer.writeDoubleLE(10+i,n+29); buffer.writeDoubleLE(90,n+37); buffer.writeDoubleLE(100,n+45);
  }
  return buffer;
}
function gpx(seconds) { return Buffer.from(`<gpx><trk><trkseg>${[0,1,2,3].map(i=>`<trkpt lat="45" lon="7"><time>${new Date((seconds+i)*1000).toISOString()}</time><speed>${10+i}</speed></trkpt>`).join('')}</trkseg></trk></gpx>`); }
test.beforeAll(async () => {
  await mkdir(directory,{recursive:true});
  execFileSync('ffmpeg',['-hide_banner','-loglevel','error','-y','-f','lavfi','-i','testsrc2=size=320x180:rate=10','-t','4','-c:v','libx264','-pix_fmt','yuv420p','-metadata','creation_time=2026-07-29T14:30:04Z',video]);
  await writeFile(resolve(directory,'matching.insgps'),insgps(start+1));
  await writeFile(resolve(directory,'different.gpx'),gpx(start+86400));
});
async function ready(page) {
  await page.goto('./'); await expect(page.locator('#status')).toContainText('Ready.',{timeout:60000});
  await page.locator('#video-file').setInputFiles(video);
  await expect(page.locator('#status')).toHaveText('dated-video.mp4');
  await page.locator('#sources summary').click();
}
test('video metadata finds INSGPS in a folder; ambiguous files require a choice',async ({page})=>{
  await ready(page);
  await page.locator('#activity-folder').setInputFiles(directory);
  await expect(page.locator('#activity-status')).toHaveText('4 activity samples linked');
  await page.locator('#activity-file').setInputFiles([
    {name:'matching.insgps',mimeType:'application/octet-stream',buffer:insgps(start+1)},
    {name:'same.gpx',mimeType:'application/gpx+xml',buffer:gpx(start+1)}]);
  await expect(page.locator('#activity-status')).toHaveText('Several activities match. Choose one:');
  await expect(page.locator('#activity-candidates button')).toHaveCount(2);
  await page.locator('#activity-candidates button').filter({hasText:'same.gpx'}).click();
  await expect(page.locator('#activity-status')).toHaveText('4 activity samples linked');
  await page.locator('#activity-offset').fill('1'); await page.locator('#activity-offset').dispatchEvent('change');
  await expect.poll(()=>page.evaluate(()=>Object.values(JSON.parse(localStorage.getItem('actionlay.web.v1')).preferences.videoSources)[0].offset)).toBe(1);
});
test('batch with no match warns; a single file aligns starts and translates warning',async ({page})=>{
  await ready(page);
  await page.locator('#activity-file').setInputFiles([{name:'wrong.gpx',mimeType:'application/gpx+xml',buffer:gpx(start+86400)},{name:'wrong.insgps',mimeType:'application/octet-stream',buffer:insgps(start+86400)}]);
  await expect(page.locator('#activity-status')).toContainText('No activity matches');
  await page.locator('#activity-file').setInputFiles({name:'wrong.insgps',mimeType:'application/octet-stream',buffer:insgps(start+86400)});
  await expect(page.locator('#activity-status')).toContainText('Starts were aligned');
  await page.locator('summary[data-i18n="Preferences"]').click();
  await page.locator('#language').selectOption('it');
  await expect(page.locator('#activity-status')).toContainText('Gli inizi sono stati allineati');
  await page.locator('#activity-unlink').click(); await expect(page.locator('#activity-status')).toBeEmpty();
});
test('WASM preserves external telemetry through camera updates and honors offset',async ({page})=>{
  await ready(page);
  const result=await page.evaluate(async bytes=>{
    const module=await import('/web/pkg/actionlay_web.js'); await module.default();
    const core=new module.Core(); core.reset_video(4);
    core.load_layout(new TextEncoder().encode(JSON.stringify({version:1,nodes:[{type:'metric',metric:'speed',pos:[50,50],size:100}]})),'speed.json');
    core.configure('metric',false,'en'); core.set_video_metadata_utc('2026-07-29T14:30:04Z');
    const summary=JSON.parse(core.activity_summary('ride.insgps',new Uint8Array(bytes)));
    const empty=Array.from(core.render(1.5,320,180));
    const linked=JSON.parse(core.link_activity('ride.insgps',new Uint8Array(bytes),0));
    const pixels=Array.from(core.render(1.5,320,180));
    core.update_telemetry(); const updated=Array.from(core.render(1.5,320,180));
    core.finish_telemetry(4); const finished=Array.from(core.render(1.5,320,180));
    core.set_activity_offset(1); const shifted=Array.from(core.render(1.5,320,180));
    core.unlink_activity(); const unlinked=Array.from(core.render(1.5,320,180));
    core.free(); return {summary,linked,empty,pixels,updated,finished,shifted,unlinked};
  },[...insgps(start+1)]);
  expect(result.summary.matches).toBe(true); expect(result.linked.fallback).toBe(false);
  expect(result.pixels).not.toEqual(result.empty); expect(result.updated).toEqual(result.pixels);
  expect(result.finished).toEqual(result.pixels); expect(result.shifted).toEqual(result.empty); expect(result.unlinked).toEqual(result.empty);
});
