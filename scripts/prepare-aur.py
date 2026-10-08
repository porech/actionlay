#!/usr/bin/env python3
"""Generate actionlay-bin's AUR recipe from a checksum-verified Linux archive."""
import argparse
import hashlib
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent.parent


def digest(path):
    checksum = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            checksum.update(chunk)
    return checksum.hexdigest()


def prepare(version, archive, output, checksums=None):
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version):
        raise ValueError('AUR recipes require a stable numeric version')
    name = f'actionlay-{version}-linux-x64.tar.gz'
    if archive.name != name:
        raise ValueError(f'Expected archive {name}')
    sha = digest(archive)
    if checksums is not None:
        entries = [line.split() for line in checksums.read_text().splitlines()]
        matches = [parts[0] for parts in entries if len(parts) == 2 and parts[1].lstrip('*') == name]
        if matches != [sha]:
            raise ValueError('Archive does not match the release SHA256SUMS')
    source = (ROOT / 'crates/app/src/integration/linux.rs').read_text()
    mime = source.split('const MIME_XML: &str = r#"', 1)[1].split('"#;', 1)[0].encode()
    template = (ROOT / 'packaging/aur/PKGBUILD.in').read_text()
    for key, value in {'VERSION': version, 'ARCHIVE_SHA256': sha,
                       'MIME_SHA256': hashlib.sha256(mime).hexdigest()}.items():
        template = template.replace(f'@{key}@', value)
    output.mkdir(parents=True, exist_ok=True)
    (output / 'PKGBUILD').write_text(template)
    (output / 'actionlay-camera-video.xml').write_bytes(mime)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--version', required=True)
    parser.add_argument('--archive', type=pathlib.Path, required=True)
    parser.add_argument('--output', type=pathlib.Path, required=True)
    parser.add_argument('--checksums', type=pathlib.Path,
                        help='Mandatory for published release recipes; CI can hash its own archive')
    args = parser.parse_args()
    prepare(args.version, args.archive, args.output, args.checksums)


if __name__ == '__main__':
    main()
