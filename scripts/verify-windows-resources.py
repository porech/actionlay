#!/usr/bin/env python3
"""Verify the built PE contains the committed icon frames and version resources."""
import pathlib
import struct
import sys

binary = pathlib.Path(sys.argv[1]).read_bytes()
u16 = lambda offset: struct.unpack_from('<H', binary, offset)[0]
u32 = lambda offset: struct.unpack_from('<I', binary, offset)[0]
pe = u32(0x3c)
assert binary[pe:pe + 4] == b'PE\0\0', 'not a PE executable'
sections_count = u16(pe + 6)
optional_size = u16(pe + 20)
optional = pe + 24
assert u16(optional) in (0x10b, 0x20b), 'unknown optional header'
directories = optional + (112 if u16(optional) == 0x20b else 96)
resource_rva = u32(directories + 16)
sections = []
for i in range(sections_count):
    offset = optional + optional_size + i * 40
    sections.append((u32(offset + 12), max(u32(offset + 8), u32(offset + 16)), u32(offset + 20)))
def address(rva):
    for start, length, file_offset in sections:
        if start <= rva < start + length:
            return file_offset + rva - start
    raise AssertionError(f'RVA {rva:x} not mapped')
base = address(resource_rva)
def entries(relative):
    offset = base + relative
    count = u16(offset + 12) + u16(offset + 14)
    return [(u32(offset + 16 + i * 8), u32(offset + 20 + i * 8)) for i in range(count)]
def payloads(relative):
    result = []
    for _, value in entries(relative):
        if value & 0x80000000:
            result.extend(payloads(value & 0x7fffffff))
        else:
            offset = base + value
            start = address(u32(offset))
            result.append(binary[start:start + u32(offset + 4)])
    return result
resources = {kind: payloads(value & 0x7fffffff) for kind, value in entries(0) if not kind & 0x80000000}
assert {3, 14, 16}.issubset(resources), 'icon/group/version resource missing'
icon_dir = pathlib.Path(__file__).resolve().parent.parent / 'assets/icons'
for size in [16, 24, 32, 48, 64, 128, 256]:
    assert (icon_dir / f'actionlay-{size}.png').read_bytes() in resources[3], f'{size}px icon missing'
assert any('ActionLay'.encode('utf-16le') in item for item in resources[16]), 'product version metadata missing'
print('Windows executable contains all seven gecko icon sizes and ActionLay version metadata')
