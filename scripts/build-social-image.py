#!/usr/bin/env python3
"""Compose the GitHub preview using the existing vector gecko (stdlib only)."""
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / 'assets/social'
OUT.mkdir(exist_ok=True)
ET.register_namespace('', 'http://www.w3.org/2000/svg')
gecko = ET.fromstring((ROOT / 'assets/icons/gecko.svg').read_text())
subject = ''.join(ET.tostring(c, encoding='unicode') for c in gecko if not c.tag.endswith('title'))
svg = '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="640" viewBox="0 0 1280 640">
<title>ActionLay — Your videos. Your telemetry. No strings attached.</title>
<defs><linearGradient id="background" x2="90%" y2="100%"><stop stop-color="#234c4b"/><stop offset="1" stop-color="#0c252d"/></linearGradient><radialGradient id="glow"><stop stop-color="#91b6a2" stop-opacity=".2"/><stop offset="1" stop-color="#91b6a2" stop-opacity="0"/></radialGradient></defs>
<rect width="1280" height="640" fill="url(#background)"/>
<circle cx="1010" cy="320" r="360" fill="url(#glow)"/>
<g fill="none" stroke="#91b6a2" stroke-opacity=".1" stroke-width="2"><path d="M690 510C850 340 1020 470 1340 190M690 540C850 370 1020 500 1340 220M690 570C850 400 1020 530 1340 250M690 600C850 430 1020 560 1340 280"/></g>
<text x="80" y="128" font-family="Roboto" font-weight="700" font-size="64" fill="#f5f6f2">ActionLay</text>
<rect x="82" y="157" width="76" height="5" rx="2.5" fill="#e5bd65"/>
<g font-family="Roboto" font-weight="700" font-size="58" fill="#f5f6f2">
<text x="80" y="270">Your videos.</text>
<text x="80" y="345">Your telemetry.</text>
<text x="80" y="420" fill="#e5bd65">No strings attached.</text>
</g>
<text x="82" y="514" font-family="Roboto" font-size="23" fill="#c9ddd2">Action-camera video with</text>
<text x="82" y="546" font-family="Roboto" font-size="23" fill="#c9ddd2">customizable telemetry overlays.</text>
<svg x="734" y="95" width="490" height="490" viewBox="0 0 1024 1024">''' + subject + '''</svg>
</svg>'''
(OUT / 'github-preview.svg').write_text(svg)
subprocess.run(['cargo', 'run', '-p', 'actionlay-render', '--example', 'render-asset', '--', str(OUT / 'github-preview.svg'), str(OUT / 'github-preview.png')], cwd=ROOT, check=True)
assert (OUT / 'github-preview.png').stat().st_size < 1_000_000, 'GitHub preview must be below 1 MB'
