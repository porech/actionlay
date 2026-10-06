#!/usr/bin/env python3
"""Download native-camera validation samples locally; never redistribute them.

DJI: Gyroflow README's public test-data folder.
Insta360: 360 Rumors ONE X2 review, personal use only / no redistribution.
Uses only Python's standard library. Downloads total about 2.47 GB.
"""
import argparse
import hashlib
from html.parser import HTMLParser
from pathlib import Path
import urllib.parse
import urllib.request

SAMPLES = {
    "dji": ("1s8BkMyyFq1aqnfIGlyRSvhm0KFXzdYbW", "dji-avata.mp4", "7d4379e96656434c324706b22e3ffa00d06a2c1c1894590f12da7db633f017b6"),
    "insta360": ("1y1CoGarqT3wfpB6Ws7vyFLkD94URvy0T", "insta360-onex2.insv", "ca081684b2363792b70f4247cc76f3b24f5cb72b25b1809ebbb74ec02dd2aa27"),
}

class DownloadForm(HTMLParser):
    def __init__(self):
        super().__init__()
        self.action = ""
        self.fields = {}
    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "form":
            self.action = attrs.get("action", "")
        if tag == "input" and attrs.get("name"):
            self.fields[attrs["name"]] = attrs.get("value", "")

def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()

def download(file_id, target, expected):
    url = "https://drive.google.com/uc?" + urllib.parse.urlencode({"export": "download", "id": file_id})
    response = urllib.request.urlopen(url, timeout=60)
    first = response.read(1024)
    if response.headers.get_content_type() == "text/html":
        form = DownloadForm()
        form.feed((first + response.read(256 * 1024)).decode("utf-8"))
        response.close()
        parsed = urllib.parse.urlparse(form.action)
        if parsed.scheme != "https" or parsed.hostname != "drive.usercontent.google.com":
            raise RuntimeError("Public download unavailable or unexpected confirmation form")
        response = urllib.request.urlopen(form.action + "?" + urllib.parse.urlencode(form.fields), timeout=60)
        first = response.read(1024)
    temporary = target.with_suffix(target.suffix + ".part")
    try:
        with response, temporary.open("wb") as output:
            output.write(first)
            while chunk := response.read(1024 * 1024):
                output.write(chunk)
        if digest(temporary) != expected:
            raise RuntimeError(f"Checksum mismatch: {target.name}")
        temporary.replace(target)
    finally:
        temporary.unlink(missing_ok=True)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", choices=SAMPLES)
    parser.add_argument("--check", action="store_true", help="Verify existing samples without downloading")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent / "samples" / "cameras"
    root.mkdir(parents=True, exist_ok=True)
    for kind, (file_id, name, expected) in SAMPLES.items():
        if args.only and kind != args.only:
            continue
        target = root / name
        if target.exists() and digest(target) == expected:
            print(f"{name}: verified")
            continue
        if args.check:
            raise RuntimeError(f"Missing or modified sample: {name}")
        print(f"Downloading {name}…", flush=True)
        download(file_id, target, expected)
        print(f"{name}: verified")

if __name__ == "__main__":
    main()
