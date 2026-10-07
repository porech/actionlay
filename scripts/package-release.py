#!/usr/bin/env python3
"""Package Windows/Linux binaries or write shared macOS bundle notices."""
import argparse
import os
import pathlib
import re
import shutil
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
VERSION = re.search(r'^version = "([^"]+)"', (ROOT / 'Cargo.toml').read_text(), re.M).group(1)

def notices(destination):
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / 'LICENSE', destination / 'LICENSE.txt')
    shutil.copy2(ROOT / 'assets/icons/LICENSE-CC-BY-SA-4.0.txt', destination / 'Artwork-CC-BY-SA-4.0.txt')
    shutil.copy2(ROOT / 'crates/render/assets/fonts/LICENSE', destination / 'Roboto-LICENSE.txt')
    shutil.copy2(ROOT / 'crates/render/assets/icons/LICENSE', destination / 'Tabler-LICENSE.txt')
    for license_file in (ROOT / 'assets/fonts/interface').glob('*-OFL.txt'):
        shutil.copy2(license_file, destination / f'Noto-{license_file.name}')
    sha = os.environ.get('GITHUB_SHA') or subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    version = VERSION
    (destination / 'SOURCE.txt').write_text(
        f'ActionLay {version}\nBuild commit: {sha}\n'
        f'Complete ActionLay source and build scripts:\nhttps://github.com/porech/actionlay/tree/{sha}\n'
        f'https://github.com/porech/actionlay/archive/{sha}.tar.gz\n\n'
        'ActionLay is GPL-3.0-or-later. FFmpeg is built with GPL enabled, never nonfree.\n'
        'ActionLay gecko photograph and artwork — Alessandro Rinaldi.\n'
        'Artwork is available under CC BY-SA 4.0 or GPL-3.0-or-later, at your option.\n'
        f'Artwork source and provenance: https://github.com/porech/actionlay/tree/{sha}/assets/icons\n'
        f'The release includes third-party-sources-{version}.tar.gz with the exact FFmpeg/x264/x265 sources.\n'
        'Their source revisions and download locations\n'
        'are in scripts/ffmpeg-version.env, scripts/build-ffmpeg.sh and scripts/build-encoders.sh.\n'
        'The Rust dependency versions are pinned in Cargo.lock. Roboto is Apache-2.0;\n'
        'Tabler icons are MIT. Noto interface fonts are SIL-OFL-1.1.\n'
        'Their notices are included alongside this file.\n', encoding='utf-8')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--notices', type=pathlib.Path)
    parser.add_argument('--target', choices=['windows', 'linux'])
    parser.add_argument('--binaries', type=pathlib.Path)
    parser.add_argument('--output', type=pathlib.Path, default=ROOT / 'dist')
    args = parser.parse_args()
    if args.notices:
        notices(args.notices)
        return
    if not args.target or not args.binaries:
        parser.error('--target and --binaries are required unless --notices is used')
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='actionlay-package-') as temporary:
        stage = pathlib.Path(temporary) / 'ActionLay'
        notices(stage / 'Licenses')
        suffix = '.exe' if args.target == 'windows' else ''
        for name in ['actionlay', 'actionlay-telemetry']:
            source = args.binaries / (name + suffix)
            shutil.copy2(source, stage / source.name)
            (stage / source.name).chmod(0o755)
        if args.target == 'windows':
            destination = args.output / f'actionlay-{VERSION}-windows-x64.zip'
            with zipfile.ZipFile(destination, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
                for file in sorted(stage.rglob('*')):
                    if file.is_file():
                        archive.write(file, file.relative_to(stage.parent))
        else:
            shutil.copy2(ROOT / 'assets/icons/actionlay-256.png', stage / 'actionlay.png')
            (stage / 'actionlay.desktop').write_text(
                '[Desktop Entry]\nType=Application\nName=ActionLay\n'
                'Comment=Action-camera telemetry dashboards\nExec=actionlay %f\n'
                'Icon=actionlay\nTerminal=false\nCategories=AudioVideo;Video;\n'
                'MimeType=video/mp4;video/quicktime;video/x-actionlay-lrv;video/x-actionlay-insv;\n', encoding='utf-8')
            destination = args.output / f'actionlay-{VERSION}-linux-x64.tar.gz'
            with tarfile.open(destination, 'w:gz') as archive:
                archive.add(stage, arcname='ActionLay')
        print(destination)

if __name__ == '__main__':
    main()
