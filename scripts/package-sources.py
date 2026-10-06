#!/usr/bin/env python3
"""Package the exact pinned FFmpeg/x264/x265 source trees used by releases."""
import io
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parent.parent
encoders = (ROOT / 'scripts/build-encoders.sh').read_text()
ffmpeg_tag = re.search(r'^FFMPEG_TAG=(.+)$', (ROOT / 'scripts/ffmpeg-version.env').read_text(), re.M).group(1).strip('"\'')
projects = [
    ('ffmpeg', 'https://git.ffmpeg.org/ffmpeg.git', ffmpeg_tag, ROOT / f'third_party/src/ffmpeg-{ffmpeg_tag}'),
    ('x264', 'https://github.com/mirror/x264.git', re.search(r'^X264_REV=(\w+)', encoders, re.M).group(1), ROOT / 'third_party/src/x264'),
    ('x265', 'https://github.com/videolan/x265.git', re.search(r'^X265_REV=(\w+)', encoders, re.M).group(1), ROOT / 'third_party/src/x265'),
]
output = ROOT / 'dist/third-party-sources.tar.gz'
output.parent.mkdir(exist_ok=True)
sha = os.environ.get('GITHUB_SHA') or subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
with tempfile.TemporaryDirectory(prefix='actionlay-source-') as directory, tarfile.open(output, 'w:gz', compresslevel=9) as bundle:
    directory = Path(directory)
    for name, url, revision, checkout in projects:
        if not (checkout / '.git').exists():
            checkout = directory / name
            subprocess.run(['git', 'init', '-q', str(checkout)], check=True)
            subprocess.run(['git', '-C', str(checkout), 'fetch', '--depth=1', url, revision], check=True)
            revision = 'FETCH_HEAD'
        archive = directory / f'{name}.tar'
        with archive.open('wb') as target:
            subprocess.run(['git', '-C', str(checkout), 'archive', '--format=tar', f'--prefix={name}/', revision], stdout=target, check=True)
        with tarfile.open(archive) as source:
            for member in source:
                bundle.addfile(member, source.extractfile(member) if member.isfile() else None)
    for name in ['scripts/build-ffmpeg.sh', 'scripts/build-encoders.sh', 'scripts/ffmpeg-version.env', 'rust-toolchain.toml']:
        bundle.add(ROOT / name, arcname=f'actionlay-build/{name}')
    text = f'ActionLay build commit: {sha}\nSource and complete build instructions: https://github.com/porech/actionlay/tree/{sha}\n\n'
    text += '\n'.join(f'{name}: {revision}\n{url}' for name, url, revision, _ in projects)
    text += '\n\nSource trees are pristine pinned revisions. The included encoder build script\napplies the x265 CMake compatibility corrections before compiling.\n'
    info = tarfile.TarInfo('BUILD-SOURCES.txt')
    payload = text.encode('utf-8')
    info.size = len(payload)
    bundle.addfile(info, io.BytesIO(payload))
print(output)
