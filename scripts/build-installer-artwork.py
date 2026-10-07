#!/usr/bin/env python3
"""Compose installer branding from the vector logo and render opaque BMPs."""
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / 'assets/installer'
OUT.mkdir(exist_ok=True)
ET.register_namespace('', 'http://www.w3.org/2000/svg')
gecko = ET.fromstring((ROOT / 'assets/icons/gecko.svg').read_text())
subject = ''.join(ET.tostring(c, encoding='unicode') for c in gecko if not c.tag.endswith('title'))
sidebar = '''<svg xmlns="http://www.w3.org/2000/svg" width="656" height="1256" viewBox="0 0 656 1256">
<title>ActionLay installer sidebar</title>
<defs><linearGradient id="panel" x2="85%" y2="100%"><stop stop-color="#234c4b"/><stop offset="1" stop-color="#0c252d"/></linearGradient><radialGradient id="glow"><stop stop-color="#90b59f" stop-opacity=".22"/><stop offset="1" stop-color="#90b59f" stop-opacity="0"/></radialGradient></defs>
<rect width="656" height="1256" fill="url(#panel)"/>
<circle cx="350" cy="570" r="390" fill="url(#glow)"/>
<g fill="none" stroke="#91b6a2" stroke-opacity=".10" stroke-width="2">
<path d="M-90 730C130 500 440 700 750 400M-90 770C130 540 440 740 750 440M-90 810C130 580 440 780 750 480M-90 850C130 620 440 820 750 520M-90 890C130 660 440 860 750 560"/>
</g>
<text x="56" y="138" font-family="Roboto" font-weight="700" font-size="82" fill="#f5f6f2">ActionLay</text>
<rect x="58" y="177" width="84" height="5" rx="2.5" fill="#e5bd65"/>
<svg x="16" y="268" width="624" height="624" viewBox="0 0 1024 1024">''' + subject + '''</svg>
<path d="M58 1103C101 1103 111 1060 151 1060S203 1134 248 1134S290 1088 332 1088" fill="none" stroke="#91b6a2" stroke-width="8" stroke-linecap="round"/>
<path d="M332 1088C373 1088 387 1032 426 1032S472 1070 523 1070H598" fill="none" stroke="#e5bd65" stroke-width="8" stroke-linecap="round"/>
<circle cx="332" cy="1088" r="15" fill="#f5f6f2" stroke="#234c4b" stroke-width="5"/>
</svg>'''
(OUT / 'wizard.svg').write_text(sidebar)
icon = ET.fromstring((ROOT / 'assets/icons/actionlay.svg').read_text())
icon.set('width', '256'); icon.set('height', '256')
icon.insert(0, ET.Element('{http://www.w3.org/2000/svg}rect', {'width':'1024','height':'1024','fill':'#ffffff'}))
(OUT / 'wizard-small.svg').write_text(ET.tostring(icon, encoding='unicode'))
for name in ['wizard', 'wizard-small']:
    subprocess.run(['cargo', 'run', '-p', 'actionlay-render', '--example', 'render-asset', '--', str(OUT / f'{name}.svg'), str(OUT / f'{name}.bmp')], cwd=ROOT, check=True)
