#!/usr/bin/env python3
"""Check that Pages serves the released index, entry assets and WASM runtime."""
import hashlib
from html.parser import HTMLParser
from pathlib import Path
import sys
import time
import urllib.error
import urllib.parse
import urllib.request


class EntryAssets(HTMLParser):
    def __init__(self):
        super().__init__()
        self.paths = ['index.html', 'pkg/actionlay_web.js', 'pkg/actionlay_web_bg.wasm']

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        path = attrs.get('src') if tag == 'script' else attrs.get('href') if tag == 'link' and attrs.get('rel') == 'stylesheet' else None
        if path and not urllib.parse.urlsplit(path).scheme and not path.startswith(('/', '//')):
            self.paths.append(path)


def verify_browser(source, url, attempts=20, delay=15):
    source = Path(source)
    parser = EntryAssets()
    parser.feed((source / 'index.html').read_text())
    expected = {path: hashlib.sha256((source / path).read_bytes()).digest()
                for path in dict.fromkeys(parser.paths)}
    url = url.rstrip('/') + '/'
    last_error = ''
    for attempt in range(attempts):
        try:
            for path, digest in expected.items():
                request = urllib.request.Request(urllib.parse.urljoin(url, path), headers={
                    'Cache-Control': 'no-cache',
                    'User-Agent': 'ActionLay-Pages-verification',
                })
                with urllib.request.urlopen(request, timeout=20) as response:
                    actual = hashlib.sha256(response.read()).digest()
                if actual != digest:
                    raise ValueError(f'{path} differs from the released bundle')
            print(f'Published browser verified: {len(expected)} files match the stable bundle.', flush=True)
            return
        except (OSError, ValueError, urllib.error.URLError) as error:
            last_error = str(error)
            if isinstance(error, urllib.error.HTTPError):
                error.close()
            print(f'Waiting for Pages ({attempt + 1}/{attempts}): {last_error}', flush=True)
            if attempt + 1 < attempts:
                time.sleep(delay)
    raise RuntimeError(f'Pages did not serve the released browser bundle: {last_error}')


if __name__ == '__main__':
    verify_browser(sys.argv[1], sys.argv[2])
