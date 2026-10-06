#!/usr/bin/env python3
"""Write Finder layout metadata directly, without launching Finder in CI."""
import pathlib
import sys
from ds_store import DSStore
from mac_alias import Alias

volume = pathlib.Path(sys.argv[1])
background = volume / '.background/background.png'
with DSStore.open(str(volume / '.DS_Store'), 'w+') as store:
    store['.']['bwsp'] = {
        'ShowStatusBar': False, 'ShowTabView': False, 'ShowToolbar': False,
        'ShowPathbar': False, 'ShowSidebar': False, 'ContainerShowSidebar': False,
        'WindowBounds': '{{180, 160}, {720, 480}}', 'SidebarWidth': 0,
    }
    store['.']['icvp'] = {
        'viewOptionsVersion': 1, 'backgroundType': 2,
        'backgroundImageAlias': Alias.for_file(str(background)).to_bytes(),
        'iconSize': 112.0, 'gridSpacing': 100.0, 'gridOffsetX': 0.0,
        'gridOffsetY': 0.0, 'arrangeBy': 'none', 'showIconPreview': True,
        'showItemInfo': False, 'labelOnBottom': True, 'textSize': 14.0,
    }
    store['.']['vSrn'] = ('long', 1)
    store['.']['icvl'] = ('type', b'icnv')
    store['ActionLay.app']['Iloc'] = (181, 246)
    store['Applications']['Iloc'] = (539, 246)
