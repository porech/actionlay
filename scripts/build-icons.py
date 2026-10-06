#!/usr/bin/env python3
"""Render the SVG masters and build multi-resolution ICO/ICNS (stdlib only)."""
import pathlib
import struct
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
ICONS = ROOT / 'assets' / 'icons'
subprocess.run([__import__('sys').executable, str(ROOT / 'scripts/compose-icons.py')], check=True)
subprocess.run(['cargo', 'run', '-p', 'actionlay-render', '--example', 'build-icons'], cwd=ROOT, check=True)

sizes = [16, 24, 32, 48, 64, 128, 256]
frames = [(size, (ICONS / f'actionlay-{size}.png').read_bytes()) for size in sizes]
header = struct.pack('<HHH', 0, 1, len(frames))
offset = 6 + 16 * len(frames)
entries = bytearray()
for size, data in frames:
    entries.extend(struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0, 1, 32, len(data), offset))
    offset += len(data)
(ICONS / 'actionlay.ico').write_bytes(header + entries + b''.join(data for _, data in frames))

# Modern macOS ICNS types store PNG data, including the Retina representations.
for name in ['actionlay', 'dmg']:
    chunks = bytearray()
    for kind, size in [('icp4', 16), ('icp5', 32), ('icp6', 64), ('ic07', 128), ('ic08', 256),
                       ('ic09', 512), ('ic10', 1024), ('ic11', 32), ('ic12', 64), ('ic13', 256), ('ic14', 512)]:
        data = (ICONS / f'{name}-{size}.png').read_bytes()
        chunks.extend(kind.encode('ascii') + struct.pack('>I', len(data) + 8) + data)
    (ICONS / f'{name}.icns').write_bytes(b'icns' + struct.pack('>I', len(chunks) + 8) + chunks)
print('Generated assets/icons/actionlay.ico, actionlay.icns and dmg.icns')
