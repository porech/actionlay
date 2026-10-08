#!/usr/bin/env python3
"""Protect against successful deployments that keep serving an older bundle."""
from contextlib import contextmanager, redirect_stdout
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
from pathlib import Path
import runpy
import tempfile
from threading import Thread
import unittest

verify_browser = runpy.run_path(str(Path(__file__).with_name('verify-browser.py')))['verify_browser']


@contextmanager
def site(files):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            path = self.path.removeprefix('/web/')
            if path not in files:
                self.send_error(404)
                return
            self.send_response(200)
            self.end_headers()
            self.wfile.write(files[path])

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f'http://127.0.0.1:{server.server_port}/web/'
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


class PublishedBrowserTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.source = Path(self.directory.name)
        self.files = {
            'index.html': b'<script src="./assets/entry.js"></script><link rel="stylesheet" href="./assets/style.css">',
            'assets/entry.js': b'current entry',
            'assets/style.css': b'current stylesheet',
            'pkg/actionlay_web.js': b'current bindings',
            'pkg/actionlay_web_bg.wasm': b'current runtime',
        }
        for path, data in self.files.items():
            output = self.source / path
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_bytes(data)

    def check(self, files):
        with site(files) as url, redirect_stdout(io.StringIO()):
            verify_browser(self.source, url, attempts=1, delay=0)

    def test_exact_bundle_succeeds(self):
        self.check(self.files)

    def test_old_index_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'index.html differs'):
            self.check({**self.files, 'index.html': b'previous deploy'})

    def test_old_runtime_with_new_index_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'actionlay_web_bg.wasm differs'):
            self.check({**self.files, 'pkg/actionlay_web_bg.wasm': b'previous runtime'})

    def test_missing_web_directory_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, '404'):
            self.check({})


if __name__ == '__main__':
    unittest.main()
