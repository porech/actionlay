#!/usr/bin/env python3
"""Check the final read-only DMG and export its alias for native macOS verification."""
from pathlib import Path
import sys
from ds_store import DSStore
from mac_alias import Alias

volume = Path(sys.argv[1]).resolve()
with DSStore.open(str(volume / '.DS_Store'), 'r') as store:
    options = store['.']['icvp']
    assert options['backgroundType'] == 2
    assert store['.']['icvl'] == (b'type', b'icnv')
    assert store['ActionLay.app']['Iloc'] == (181, 246)
    assert store['Applications']['Iloc'] == (539, 246)
    record = options['backgroundImageAlias']
    alias = Alias.from_bytes(record)
    assert alias.target.posix_path in (b'/.background/background.png', '/.background/background.png')
    assert alias.volume.posix_path in (b'/Volumes/ActionLay', '/Volumes/ActionLay')
    Path(sys.argv[2]).write_bytes(record)
assert (volume / '.background/background.png').is_file()
assert (volume / 'ActionLay.app').is_dir()
assert (volume / 'Applications').is_symlink()
print('Verified Finder layout and portable background alias')
