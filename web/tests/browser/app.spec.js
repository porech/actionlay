import { test, expect } from '@playwright/test';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { gpmfFixture } from '../gpmf-fixture.js';
const telemetryFixture = resolve('test-results/telemetry.mp4');
const fixture = resolve('test-results/source.mp4');
test.beforeAll(async () => {
  await mkdir('test-results', { recursive: true });
  execFileSync('ffmpeg', ['-hide_banner','-loglevel','error','-y','-f','lavfi','-i','testsrc2=size=320x180:rate=15','-f','lavfi','-i','sine=frequency=440:sample_rate=48000','-t','2','-c:v','libx264','-pix_fmt','yuv420p','-c:a','aac',fixture]);
  const metadata=resolve('test-results/metadata.mp4');
  await writeFile(metadata,gpmfFixture());
  execFileSync('ffmpeg',['-hide_banner','-loglevel','error','-y','-i',fixture,'-i',metadata,'-map','0','-map','1:0','-c','copy','-tag:d','gpmd',telemetryFixture]);
});
async function ready(page) {
  await page.goto('./');
  await expect(page.locator('#status')).toContainText('Ready.', { timeout: 60_000 });
  await page.locator('summary').click();
  await page.locator('#maps').uncheck();
}
test('persist preferences, edit/import/download layout with assets, restore after reload', async ({ page }) => {
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  await ready(page);
  await page.locator('#units').selectOption('imperial');
  await page.locator('#layout-file').setInputFiles({ name: 'Ride.ovl.json', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify({version:1,name:'Ride',nodes:[{type:'text',text:'Ride'}]})) });
  await expect(page.locator('#status')).toContainText('Layout imported');
  await page.locator('#edit').click();
  await expect(page.locator('#editor')).toBeVisible();
  await page.locator('#asset-file').setInputFiles({ name:'test.png',mimeType:'image/png',buffer:await readFile('../assets/icons/actionlay-256.png') });
  await page.locator('#editor-save').click();
  await expect(page.locator('#status')).toContainText('saved and applied');
  const saved = await page.evaluate(() => JSON.parse(localStorage.getItem('actionlay.web.v1')));
  expect(saved.layouts).toHaveLength(1); expect(saved.layouts[0].bytes.length).toBeGreaterThan(1000);
  const downloadEvent = page.waitForEvent('download'); await page.locator('#editor-download').click();
  const download = await downloadEvent; const path = await download.path(); const bytes = await readFile(path);
  expect(bytes.subarray(0,2).toString()).toBe('PK');
  await page.locator('#editor-close').click();
  await page.reload(); await expect(page.locator('#status')).toContainText('Ready.');
  await expect(page.locator('#units')).toHaveValue('imperial');
  await expect(page.locator('#layouts option:checked')).toHaveText('Ride');
  await page.locator('#layout-file').setInputFiles({name:'Roundtrip.actionlay-layout',mimeType:'application/zip',buffer:bytes});
  await expect(page.locator('#status')).toContainText('Layout imported');
  expect(errors).toEqual([]);
});
test('play, seek and export a trimmed video with overlay and audio', async ({ page }) => {
  const errors=[]; page.on('pageerror', e=>errors.push(e.message));
  await ready(page);
  await page.locator('#video-file').setInputFiles(fixture);
  await expect(page.locator('#status')).toHaveText('source.mp4', {timeout:60000});
  await page.evaluate(async () => { const v=document.querySelector('video'); await v.play(); });
  await page.waitForTimeout(250);
  await page.evaluate(() => { const v=document.querySelector('video'); v.pause(); v.currentTime=1; });
  await expect.poll(() => page.locator('#overlay').evaluate(c=>c.getContext('2d').getImageData(0,0,c.width,c.height).data.some((v,i)=>i%4===3&&v>0))).toBe(true);
  await page.locator('#layout-file').setInputFiles({name:'Export.ovl.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify({version:1,name:'Export',nodes:[{type:'frame',size:[300,300],fill:'#ff00ff'}]}))});
  await expect(page.locator('#status')).toContainText('Layout imported');
  await page.locator('#start').fill('0.5'); await page.locator('#end').fill('1.5');
  // Exercise the download fallback independently of OS-specific native save pickers.
  await page.evaluate(() => { window.showSaveFilePicker = undefined; });
  const downloadEvent = page.waitForEvent('download', {timeout:90000});
  await page.locator('#export').click();
  await expect(page.locator('#status')).toContainText('Export complete.', {timeout:90000});
  const download = await downloadEvent; const path = await download.path();
  const info=JSON.parse(execFileSync('ffprobe',['-v','error','-show_streams','-show_format','-of','json',path],{encoding:'utf8'}));
  expect(info.streams.some(s=>s.codec_type==='video' && s.codec_name==='h264')).toBe(true);
  expect(info.streams.some(s=>s.codec_type==='audio' && s.codec_name==='aac')).toBe(true);
  expect(Number(info.format.duration)).toBeGreaterThanOrEqual(.9); expect(Number(info.format.duration)).toBeLessThan(1.15);
  const pixel=execFileSync('ffmpeg',['-v','error','-i',path,'-frames:v','1','-vf','crop=2:2:20:20','-f','rawvideo','-pix_fmt','rgb24','pipe:1']);
  expect(pixel[0]).toBeGreaterThan(220); expect(pixel[1]).toBeLessThan(35); expect(pixel[2]).toBeGreaterThan(220);
  await page.locator('#mode').selectOption('solid');
  await expect(page.locator('#color-control')).toBeVisible();
  const solidEvent=page.waitForEvent('download', {timeout:90000});
  await page.locator('#export').click();
  await expect(page.locator('#status')).toContainText('Export complete.', {timeout:90000});
  const solidPath=await (await solidEvent).path();
  const green=execFileSync('ffmpeg',['-v','error','-i',solidPath,'-frames:v','1','-vf','crop=2:2:300:160','-f','rawvideo','-pix_fmt','rgb24','pipe:1']);
  expect(green[0]).toBeLessThan(35); expect(green[1]).toBeGreaterThan(220); expect(green[2]).toBeLessThan(35);
  expect(errors).toEqual([]);
});

test('stream export commits only on success; cancel preserves an existing destination', async ({ page }) => {
  await ready(page);
  await page.locator('#video-file').setInputFiles(fixture);
  await expect(page.locator('#status')).toHaveText('source.mp4');
  await page.evaluate(async () => {
    const directory = await navigator.storage.getDirectory();
    const handle = await directory.getFileHandle('export.mp4', {create:true});
    window.showSaveFilePicker = async () => handle;
  });
  await page.locator('#export').click();
  await expect(page.locator('#status')).toContainText('Export complete.');
  const size = await page.evaluate(async () => (await (await (await navigator.storage.getDirectory()).getFileHandle('export.mp4')).getFile()).size);
  expect(size).toBeGreaterThan(1000);
  await page.evaluate(async () => {
    const handle = await (await navigator.storage.getDirectory()).getFileHandle('export.mp4');
    const writable = await handle.createWritable(); await writable.write('original'); await writable.close();
  });
  await page.locator('#export').click();
  await page.locator('#cancel').click();
  await expect(page.locator('#status')).toContainText('Export cancelled.');
  expect(await page.evaluate(async () => (await (await (await navigator.storage.getDirectory()).getFileHandle('export.mp4')).getFile()).text())).toBe('original');
});

test('discarding a new draft keeps the active layout', async ({ page }) => {
  await ready(page);
  await page.locator('#layouts').selectOption('builtin:moto');
  await page.locator('#new').click();
  await expect(page.locator('#editor')).toBeVisible();
  page.once('dialog', dialog => dialog.accept());
  await page.locator('#editor-close').click();
  await expect(page.locator('#layouts')).toHaveValue('builtin:moto');
  const event = page.waitForEvent('download'); await page.locator('#save').click();
  const bytes=await readFile(await (await event).path());
  expect(bytes.subarray(0,2).toString()).toBe('PK');
});

test('fullscreen includes overlay; background appears only in solid mode with default green', async ({ page }) => {
  await ready(page);
  await expect(page.locator('#color-control')).toBeHidden();
  await page.locator('#mode').selectOption('solid');
  await expect(page.locator('#color-control')).toBeVisible();
  await expect(page.locator('#color')).toHaveValue('#00ff00');
  await expect(page.locator('.hint')).toContainText('Export to device');
  await page.locator('#video-file').setInputFiles(fixture);
  await expect(page.locator('#status')).toHaveText('source.mp4');
  await page.locator('#fullscreen').click();
  await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id)).toBe('stage');
  await expect(page.locator('#overlay')).toBeVisible();
  await expect.poll(() => page.locator('#overlay').evaluate(c=>c.getContext('2d').getImageData(0,0,c.width,c.height).data.some((v,i)=>i%4===3&&v>0))).toBe(true);
  await page.locator('#fullscreen').click();
  await expect.poll(() => page.evaluate(() => Boolean(document.fullscreenElement))).toBe(false);
});
test('browser Italian is automatic and an explicit language survives reload', async ({ browser }) => {
  const context = await browser.newContext({locale:'it-IT'});
  const page = await context.newPage();
  await page.goto('./');
  await expect(page.locator('#status')).toContainText('Pronto.', {timeout:60000});
  await expect(page.locator('html')).toHaveAttribute('lang','it');
  await expect(page.locator('header small')).toHaveText('Your videos. Your telemetry. No strings attached.');
  await expect(page.locator('#export')).toHaveText('Esporta sul dispositivo');
  await page.locator('summary').click();
  await expect(page.locator('#language')).toHaveValue('system');
  await page.locator('#language').selectOption('en');
  await expect(page.locator('html')).toHaveAttribute('lang','en');
  await page.reload();
  await expect(page.locator('#status')).toContainText('Ready.', {timeout:60000});
  await expect(page.locator('#language')).toHaveValue('en');
  await context.close();
});
test('progressive metadata renders before completion and seek prioritises the requested packets', async ({ page }) => {
  await ready(page);
  const worker=page.workers()[0];
  await worker.evaluate(async () => {
    const read=Blob.prototype.arrayBuffer;
    Blob.prototype.arrayBuffer=async function() { if(this.size<1024) await new Promise(r=>setTimeout(r,100)); return read.call(this); };
    const module=await import('/web/pkg/actionlay_web.js');
    const add=module.Core.prototype.add_packet;
    self.packetTimes=[];
    module.Core.prototype.add_packet=function(pts,...args) { self.packetTimes.push(pts); return add.call(this,pts,...args); };
  });
  await page.locator('#video-file').setInputFiles(telemetryFixture);
  await expect(page.locator('#map-loaders')).toBeVisible();
  await expect(page.locator('#metadata-progress')).toHaveCount(0);
  await expect.poll(() => worker.evaluate(() => self.packetTimes.length)).toBeGreaterThan(0);
  await expect.poll(() => page.locator('#overlay').evaluate(c=>c.getContext('2d').getImageData(0,0,c.width,c.height).data.some((v,i)=>i%4===3&&v>0))).toBe(true);
  await page.evaluate(() => { document.querySelector('video').currentTime=1.6; });
  await expect.poll(() => worker.evaluate(() => self.packetTimes.some(t=>t>=.6))).toBe(true);
  const order=await worker.evaluate(() => self.packetTimes);
  const jumped=order.findIndex(t=>t>=.6);
  expect(jumped).toBeLessThan(8);
  await expect(page.locator('#status')).toHaveText('telemetry.mp4',{timeout:30000});
  await expect(page.locator('#map-loaders')).toBeHidden({timeout:30000});
  const packets=await worker.evaluate(() => self.packetTimes);
  expect(new Set(packets).size).toBe(50);
});

test('without a route map, playback and seek read local telemetry windows only', async ({ page }) => {
  await ready(page);
  await page.locator('#layout-file').setInputFiles({name:'Simple.ovl.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify({version:1,name:'Simple',nodes:[{type:'text',text:'Simple'}]}))});
  await expect(page.locator('#status')).toContainText('Layout imported');
  const worker=page.workers()[0];
  await worker.evaluate(async () => {
    const module=await import('/web/pkg/actionlay_web.js');
    const add=module.Core.prototype.add_packet; self.packetTimes=[];
    module.Core.prototype.add_packet=function(pts,...args) {self.packetTimes.push(pts); return add.call(this,pts,...args);};
  });
  await page.locator('#video-file').setInputFiles(telemetryFixture);
  await expect(page.locator('#status')).toHaveText('telemetry.mp4');
  await expect.poll(() => worker.evaluate(() => self.packetTimes.length)).toBeGreaterThan(10);
  await page.waitForTimeout(300);
  expect(await worker.evaluate(() => self.packetTimes.length)).toBeLessThan(20);
  await expect(page.locator('#map-loaders')).toBeHidden();
  await page.evaluate(() => {document.querySelector('video').currentTime=1.6;});
  await expect.poll(() => worker.evaluate(() => self.packetTimes.some(t=>t>=1.6))).toBe(true);
  await page.waitForTimeout(300);
  expect(await worker.evaluate(() => self.packetTimes.length)).toBeLessThan(50);
});

test('dropping a video on the page opens it; invalid files do not replace it or navigate away', async ({ page }) => {
  await ready(page);
  const url = page.url();
  // Some operating systems provide an empty MIME type for dragged camera files.
  const dataTransfer = await page.evaluateHandle(base64 => {
    const transfer = new DataTransfer();
    transfer.items.add(new File([Uint8Array.from(atob(base64), c=>c.charCodeAt(0))], 'Dropped.MP4'));
    return transfer;
  }, (await readFile(fixture)).toString('base64'));
  await page.dispatchEvent('body', 'dragenter', {dataTransfer});
  await expect(page.locator('#drop-hint')).toBeVisible();
  await page.dispatchEvent('#edit', 'dragenter', {dataTransfer});
  await page.dispatchEvent('#edit', 'dragleave', {dataTransfer});
  await expect(page.locator('#drop-hint')).toBeVisible();
  await page.dispatchEvent('body', 'dragover', {dataTransfer});
  await page.dispatchEvent('body', 'drop', {dataTransfer});
  await expect(page.locator('#drop-hint')).toBeHidden();
  await expect(page.locator('#status')).toHaveText('Dropped.MP4');
  await expect(page.locator('#player-controls')).toBeVisible();
  const source=await page.locator('#video').getAttribute('src');
  const invalid=await page.evaluateHandle(() => {
    const transfer=new DataTransfer(); transfer.items.add(new File(['hello'],'notes.txt',{type:'text/plain'})); return transfer;
  });
  await page.dispatchEvent('body','drop',{dataTransfer:invalid});
  await expect(page.locator('#status')).toHaveText('Drop a video file to open it.');
  await expect(page.locator('#video')).toHaveAttribute('src',source);
  expect(page.url()).toBe(url);
  await dataTransfer.dispose(); await invalid.dispose();
});

test('controls sit below the video and fullscreen hides controls and cursor after three idle seconds', async ({ page }) => {
  await ready(page);
  await page.locator('#video-file').setInputFiles(fixture);
  await expect(page.locator('#status')).toHaveText('source.mp4');
  const area = await page.locator('#video-area').boundingBox();
  const controls = await page.locator('#player-controls').boundingBox();
  expect(controls.y).toBeGreaterThanOrEqual(area.y+area.height);
  await page.clock.install();
  await page.clock.pauseAt(new Date(Date.now()+1000));
  await page.locator('#fullscreen').click();
  await expect.poll(() => page.evaluate(() => document.fullscreenElement?.id)).toBe('stage');
  await page.mouse.move(100,100);
  await page.clock.fastForward(2999);
  await expect(page.locator('#player-controls')).toBeVisible();
  await page.clock.fastForward(1);
  await expect(page.locator('#player-controls')).toBeHidden();
  await expect(page.locator('#stage')).toHaveCSS('cursor','none');
  await expect(page.locator('#video')).toHaveCSS('cursor','none');
  // Playback changes and clicks with a stationary mouse do not reveal controls.
  await page.keyboard.press('Space');
  await page.mouse.down(); await page.mouse.up();
  await expect(page.locator('#player-controls')).toBeHidden();
  await page.mouse.move(110,100);
  await expect(page.locator('#player-controls')).toBeVisible();
  await expect(page.locator('#stage')).not.toHaveCSS('cursor','none');
  await page.clock.fastForward(2500);
  await page.mouse.move(120,100);
  await page.clock.fastForward(2500);
  await expect(page.locator('#player-controls')).toBeVisible();
  await page.clock.fastForward(500);
  await expect(page.locator('#player-controls')).toBeHidden();
  await page.mouse.move(130,100);
  await page.locator('#fullscreen').click();
  await expect.poll(() => page.evaluate(() => Boolean(document.fullscreenElement))).toBe(false);
  await page.clock.fastForward(5000);
  await expect(page.locator('#player-controls')).toBeVisible();
  await expect(page.locator('#stage')).not.toHaveCSS('cursor','none');
  const restored = await page.locator('#player-controls').boundingBox();
  const restoredArea = await page.locator('#video-area').boundingBox();
  expect(restored.y).toBeGreaterThanOrEqual(restoredArea.y+restoredArea.height);
});
