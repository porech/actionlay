#!/usr/bin/env python3
"""Compose the app tile and disk artwork around the editable gecko SVG."""
from pathlib import Path
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent / 'assets/icons'
ET.register_namespace('', 'http://www.w3.org/2000/svg')
gecko = ET.fromstring((ROOT / 'gecko.svg').read_text())
subject = ''.join(ET.tostring(child, encoding='unicode') for child in gecko if not child.tag.endswith('title'))
header = '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024" role="img">'
shadow = '<filter id="gecko-shadow" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="6" stdDeviation="4" flood-color="#07191a" flood-opacity=".25"/></filter>'
tile = '''<defs><linearGradient id="tile" x2="85%" y2="100%"><stop stop-color="#234c4b"/><stop offset="1" stop-color="#0c252d"/></linearGradient>'''+shadow+'''</defs><rect x="48" y="48" width="928" height="928" rx="207" fill="url(#tile)"/><rect x="50" y="50" width="924" height="924" rx="205" fill="none" stroke="#91b6a2" stroke-opacity=".2" stroke-width="4"/>'''
(ROOT / 'actionlay.svg').write_text(header+'<title>ActionLay</title>'+tile+'<g filter="url(#gecko-shadow)">'+subject+'</g></svg>\n')
drive = '''<defs><linearGradient id="metal" x2="15%" y2="100%"><stop stop-color="#fafafa"/><stop offset=".45" stop-color="#d8dce0"/><stop offset="1" stop-color="#8c949e"/></linearGradient><linearGradient id="edge" x2="0" y2="100%"><stop stop-color="#919ba6"/><stop offset="1" stop-color="#d5d9df"/></linearGradient>'''+shadow+'''</defs><path d="M200,155Q210,123 244,123H780Q814,123 824,155L954,778Q968,817 937,848L901,889H123L87,848Q56,817 70,778Z" fill="url(#metal)" stroke="#66727c" stroke-width="5"/><path d="M74,794Q71,838 116,843H908Q953,838 950,794L954,865Q953,909 903,914H121Q71,909 70,865Z" fill="url(#edge)" stroke="#66727c" stroke-width="4"/><path d="M242,160H782" stroke="#fff" stroke-opacity=".9" stroke-width="4"/><rect x="136" y="856" width="210" height="12" rx="6" fill="#515a63"/><circle cx="875" cy="871" r="8" fill="#6dbca1"/><path d="M225,715H800" stroke="#74818d" stroke-opacity=".3" stroke-width="3"/>'''
(ROOT / 'dmg.svg').write_text(header+'<title>ActionLay disk image</title>'+drive+'<g transform="translate(0 62) translate(512 400) scale(.84 .70) translate(-512 -512)" filter="url(#gecko-shadow)">'+subject+'</g></svg>\n')
