#!/usr/bin/env python3
"""Write Finder layout metadata directly, without launching Finder in CI."""
import pathlib
import sys
from ds_store import DSStore
from mac_alias import Alias
from mac_alias.alias import ALIAS_EJECTABLE_DISK

volume = pathlib.Path(sys.argv[1]).resolve()
background = volume / '.background/background.png'
# Alias.for_file compares the path against statfs's canonical mount point.
# /tmp is a symlink to /private/tmp on macOS; without resolve() it writes
# a bogus /../../../../tmp/... target instead of a volume-relative path.
alias = Alias.for_file(str(background))
alias.volume.disk_type = ALIAS_EJECTABLE_DISK
# Finder mounts downloaded images under /Volumes, never the CI staging path.
name = alias.volume.name
if isinstance(name, bytes):
    name = name.decode('utf-8')
alias.volume.posix_path = '/Volumes/' + name
assert alias.target.posix_path in (b'/.background/background.png', '/.background/background.png')
with DSStore.open(str(volume / '.DS_Store'), 'w+') as store:
    store['.']['bwsp'] = {
        'ShowStatusBar': False, 'ShowTabView': False, 'ShowToolbar': False,
        'ShowPathbar': False, 'ShowSidebar': False, 'ContainerShowSidebar': False,
        'WindowBounds': '{{180, 160}, {720, 480}}', 'SidebarWidth': 0,
    }
    store['.']['icvp'] = {
        'viewOptionsVersion': 1, 'backgroundType': 2,
        'backgroundColorRed': 1.0, 'backgroundColorGreen': 1.0,
        'backgroundColorBlue': 1.0,
        'backgroundImageAlias': alias.to_bytes(),
        'iconSize': 112.0, 'gridSpacing': 100.0, 'gridOffsetX': 0.0,
        'gridOffsetY': 0.0, 'arrangeBy': 'none', 'showIconPreview': True,
        'showItemInfo': False, 'labelOnBottom': True, 'textSize': 14.0,
    }
    store['.']['vSrn'] = ('long', 1)
    store['.']['icvl'] = ('type', b'icnv')
    store['ActionLay.app']['Iloc'] = (181, 246)
    store['Applications']['Iloc'] = (539, 246)
