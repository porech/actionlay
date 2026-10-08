#!/usr/bin/env python3
"""Render the project landing page and Linux installation guide."""
import html
import pathlib
import os
import re
import shutil
import sys

root = pathlib.Path(__file__).resolve().parents[2]
site = pathlib.Path(sys.argv[1])
site.mkdir(parents=True, exist_ok=True)
packages = site / 'packages'
packages.mkdir(parents=True, exist_ok=True)
shutil.copy2(root / 'assets/icons/actionlay-256.png', packages / 'actionlay.png')
web_link = ('<p><a href="../web/">Open ActionLay in your browser</a> — local video playback, telemetry dashboards, layout editing and video export.</p>\n'
            if (site / 'web/index.html').is_file() else '')
sections = []
for channel, label in [('stable', 'Stable releases'), ('nightly', 'Development builds')]:
    if not (site / channel / 'apt/InRelease').exists():
        sections.append(f'<section><h2>{label}</h2><p>Available after the first release with native packages. For now, use the development channel below.</p></section>')
        continue
    url = f'https://porech.github.io/actionlay/{channel}'
    apt = f'''sudo install -d -m 755 /etc/apt/keyrings
curl -fsSL {url}/apt/key.asc | sudo tee /etc/apt/keyrings/actionlay.asc >/dev/null
echo 'deb [arch=amd64 signed-by=/etc/apt/keyrings/actionlay.asc] {url}/apt ./' | sudo tee /etc/apt/sources.list.d/actionlay.list
sudo apt update
sudo apt install actionlay'''
    dnf = f'''sudo curl -fsSL {url}/rpm/actionlay.repo -o /etc/yum.repos.d/actionlay.repo
sudo dnf install actionlay'''
    sections.append(f'<section><h2>{label}</h2><h3>Ubuntu 22.04+, Linux Mint 21+, Debian 12+</h3><pre>{html.escape(apt)}</pre><h3>Fedora and compatible RPM distributions (glibc 2.35+)</h3><pre>{html.escape(dnf)}</pre></section>')
(packages / 'index.html').write_text('''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>ActionLay packages</title>
<style>body{font:16px/1.6 system-ui,sans-serif;background:#edf2ed;color:#153c3b;max-width:900px;margin:48px auto;padding:0 24px}header{display:flex;align-items:center;gap:24px}header img{width:112px}h1{font-size:40px;margin:0}section{background:white;border:1px solid #d3ded6;border-radius:18px;padding:24px;margin:24px 0}pre{overflow:auto;background:#102c32;color:#e3f2e7;border-radius:10px;padding:18px;font-size:13px}a{color:#226e58}footer{font-size:14px}</style>
<header><img src="actionlay.png" alt="ActionLay gecko"><div><h1>ActionLay packages</h1><p>Your videos. Your telemetry. No strings attached.</p></div></header>
<p>Signed APT and DNF repositories for 64-bit Linux. Packages add ActionLay to your applications menu and Open With without changing your default player. Choose one channel.</p>
''' + web_link + ''.join(sections) + '''<section><h2>Remove ActionLay</h2><pre>sudo apt remove actionlay\n# or\nsudo dnf remove actionlay</pre><p>To stop receiving updates, also remove the repository configuration file added above.</p></section>
<footer><a href="https://github.com/porech/actionlay">Source, licence and documentation</a> · <a href="https://github.com/porech/actionlay/releases">Windows installer and universal macOS DMG</a><p>APT and DNF verify signatures using the repository key. Original gecko photograph and artwork: Alessandro Rinaldi, CC BY-SA 4.0 or GPL-3.0-or-later.</p></footer></html>''', encoding='utf-8')

# Landing assets are separate from the browser bundle and desktop builds.
for name in ['landing.css', 'landing.js']:
    shutil.copy2(root / 'scripts/pages' / name, site / name)
shutil.copy2(root / 'assets/site/gecko.png', site / 'gecko.png')
shutil.copy2(root / 'assets/icons/actionlay-256.png', site / 'actionlay.png')
shutil.copy2(root / 'assets/social/github-preview.png', site / 'social.png')
tag = os.environ.get('ACTIONLAY_STABLE_TAG')
if tag:
    version = tag.removeprefix('v')
else:
    version = re.search(r'^version = "([^"\n]+)"', (root / 'Cargo.toml').read_text(), re.MULTILINE).group(1)
if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?', version):
    raise ValueError(f'Invalid stable release version: {version}')
landing = (root / 'scripts/pages/landing.html').read_text()
web_button = '<a class="button secondary" href="web/" target="_blank" rel="noopener noreferrer">Try it in your browser <span aria-hidden="true">→</span></a>' if (site / 'web/index.html').is_file() else ''
(site / 'index.html').write_text(landing.replace('@@VERSION@@', html.escape(version)).replace('@@WEB_LINK@@', web_button), encoding='utf-8')
