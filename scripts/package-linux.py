#!/usr/bin/env python3
"""Build native DEB/RPM packages from the tested portable Linux binaries."""
import argparse
import pathlib
import re
import runpy
import shutil
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binaries', type=pathlib.Path, required=True)
    parser.add_argument('--output', type=pathlib.Path, default=ROOT / 'dist')
    args = parser.parse_args()
    version = re.search(r'^version = "([^"]+)"', (ROOT / 'Cargo.toml').read_text(), re.M).group(1)
    # Nightly packages sort above the current stable and below the next version.
    import os
    if os.environ.get('GITHUB_REF') == 'refs/heads/main':
        version += '+git' + os.environ['GITHUB_RUN_NUMBER']
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='actionlay-linux-package-') as temporary:
        work = pathlib.Path(temporary)
        stage = work / 'stage'
        binaries = stage / 'usr/bin'
        binaries.mkdir(parents=True)
        for name in ['actionlay', 'actionlay-telemetry']:
            shutil.copy2(args.binaries / name, binaries / name)
            (binaries / name).chmod(0o755)
        share = stage / 'usr/share'
        applications = share / 'applications'
        applications.mkdir(parents=True)
        (applications / 'org.ActionLay.ActionLay.desktop').write_text(
            '[Desktop Entry]\nType=Application\nName=ActionLay\n'
            'Comment=Action-camera telemetry dashboards\nExec=actionlay %f\n'
            'Icon=actionlay\nTerminal=false\nCategories=AudioVideo;Video;\n'
            'MimeType=video/mp4;video/quicktime;video/x-actionlay-lrv;video/x-actionlay-insv;\n'
            'X-ActionLay-Managed=true\n', encoding='utf-8')
        icons = share / 'icons/hicolor/256x256/apps'
        icons.mkdir(parents=True)
        shutil.copy2(ROOT / 'assets/icons/actionlay-256.png', icons / 'actionlay.png')
        # Keep the MIME declarations identical to the portable integration.
        source = (ROOT / 'crates/app/src/integration/linux.rs').read_text()
        xml = source.split('const MIME_XML: &str = r#"', 1)[1].split('"#;', 1)[0]
        mime = share / 'mime/packages'
        mime.mkdir(parents=True)
        (mime / 'actionlay-camera-video.xml').write_text(xml, encoding='utf-8')
        notices = runpy.run_path(str(ROOT / 'scripts/package-release.py'))['notices']
        notices(share / 'doc/actionlay')
        deb = work / 'deb'
        shutil.copytree(stage, deb)
        control = deb / 'DEBIAN'
        control.mkdir()
        (control / 'control').write_text(
            f'Package: actionlay\nVersion: {version}\nArchitecture: amd64\n'
            'Maintainer: Alessandro Rinaldi\nSection: video\nPriority: optional\n'
            'Depends: libc6 (>= 2.35), libasound2, libva2, libva-drm2, libdrm2, libstdc++6\n'
            'Recommends: shared-mime-info, desktop-file-utils\n'
            'Homepage: https://github.com/porech/actionlay\n'
            'Description: Action-camera telemetry dashboards and video export\n'
            ' Visual overlay editor with GoPro, DJI/Insta360 and GPX/FIT telemetry.\n')
        refresh = '#!/bin/sh\nset -e\nupdate-mime-database /usr/share/mime >/dev/null 2>&1 || true\nupdate-desktop-database /usr/share/applications >/dev/null 2>&1 || true\nexit 0\n'
        for name in ['postinst', 'postrm']:
            (control / name).write_text(refresh)
            (control / name).chmod(0o755)
        subprocess.run(['dpkg-deb', '--build', '--root-owner-group', str(deb), str(args.output / f'actionlay_{version}_amd64.deb')], check=True)
        rpm_version = version.split('+')[0]
        rpm_release = '1' if '+' not in version else '1.git' + version.split('+git')[1]
        rpm = work / 'rpm'
        for directory in ['BUILD', 'BUILDROOT', 'RPMS', 'SOURCES', 'SPECS', 'SRPMS']:
            (rpm / directory).mkdir(parents=True)
        spec = rpm / 'SPECS/actionlay.spec'
        spec.write_text(f'''Name: actionlay
Version: {rpm_version}
Release: {rpm_release}
Summary: Action-camera telemetry dashboards and video export
License: GPL-3.0-or-later AND (CC-BY-SA-4.0 OR GPL-3.0-or-later)
URL: https://github.com/porech/actionlay
BuildArch: x86_64
Requires: glibc >= 2.35, alsa-lib, libva, libstdc++, shared-mime-info, desktop-file-utils
AutoReqProv: no
%description
Visual overlay editor with GoPro, DJI/Insta360 and GPX/FIT telemetry.
%install
mkdir -p %{{buildroot}}
cp -a {stage}/usr %{{buildroot}}/
%post
update-mime-database /usr/share/mime >/dev/null 2>&1 || :
update-desktop-database /usr/share/applications >/dev/null 2>&1 || :
%postun
update-mime-database /usr/share/mime >/dev/null 2>&1 || :
update-desktop-database /usr/share/applications >/dev/null 2>&1 || :
%files
/usr/bin/actionlay
/usr/bin/actionlay-telemetry
/usr/share/applications/org.ActionLay.ActionLay.desktop
/usr/share/icons/hicolor/256x256/apps/actionlay.png
/usr/share/mime/packages/actionlay-camera-video.xml
/usr/share/doc/actionlay
''')
        subprocess.run(['rpmbuild', '--define', f'_topdir {rpm}', '--define', '_build_id_links none', '--define', '__os_install_post %{nil}', '-bb', str(spec)], check=True)
        for package in (rpm / 'RPMS').rglob('*.rpm'):
            shutil.copy2(package, args.output / package.name)

if __name__ == '__main__':
    main()
