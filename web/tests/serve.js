// Serve the production bundle under the same subdirectory used on GitHub Pages.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
const root = resolve('dist');
createServer(async (req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
  if (!path.startsWith('/web/')) { res.writeHead(404).end(); return; }
  const file = resolve(root, path.slice(5) || 'index.html');
  if (!file.startsWith(`${root}/`)) { res.writeHead(403).end(); return; }
  try {
    const data = await readFile(file);
    const ext = file.split('.').pop();
    res.setHeader('Content-Type', ({ html: 'text/html', js: 'text/javascript', wasm: 'application/wasm', css: 'text/css', png: 'image/png' })[ext] ?? 'application/octet-stream');
    res.end(data);
  } catch { res.writeHead(404).end(); }
}).listen(4173, '127.0.0.1');
