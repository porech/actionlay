#!/usr/bin/env bash
# Build, lint, install and remove a generated recipe inside an ephemeral Arch container.
set -euo pipefail
RECIPE="$(cd "${1:?recipe directory required}" && pwd)"
ARCHIVE="$(cd "$(dirname "${2:?Linux archive required}")" && pwd)/$(basename "$2")"
OUTPUT="${3:?output directory required}"
mkdir -p "$OUTPUT"
OUTPUT="$(cd "$OUTPUT" && pwd)"
docker run --rm --platform linux/amd64 \
  -v "$RECIPE:/recipe:ro" -v "$ARCHIVE:/archive/$(basename "$ARCHIVE"):ro" \
  -v "$OUTPUT:/output" archlinux:base-devel bash -euxo pipefail -c '
    pacman --disable-sandbox -Syu --noconfirm --needed sudo namcap desktop-file-utils shared-mime-info
    # Nested Docker/emulation may lack the kernel features used by pacman’s
    # download sandbox. This setting is confined to this ephemeral test container.
    printf "\nDisableSandbox\n" >> /etc/pacman.conf
    useradd -m builder
    printf "builder ALL=(ALL) NOPASSWD: /usr/bin/pacman\n" > /etc/sudoers.d/builder
    mkdir /build
    cp /recipe/* /build/
    cp /archive/* /build/
    chown -R builder:builder /build
    cd /build
    sudo -u builder makepkg --syncdeps --noconfirm --cleanbuild
    sudo -u builder makepkg --printsrcinfo > .SRCINFO
    namcap PKGBUILD
    namcap actionlay-bin-*.pkg.tar.zst | tee /tmp/namcap.log
    if grep -E " E: " /tmp/namcap.log; then exit 1; fi
    pacman -U --noconfirm actionlay-bin-*.pkg.tar.zst
    pacman -Qo /usr/bin/actionlay
    actionlay-telemetry --version
    if ldd /usr/bin/actionlay | grep "not found"; then exit 1; fi
    desktop-file-validate /usr/share/applications/org.ActionLay.ActionLay.desktop
    grep -Fx "X-ActionLay-Managed=true" /usr/share/applications/org.ActionLay.ActionLay.desktop
    grep -F "video/x-actionlay-lrv:*.lrv" /usr/share/mime/globs2
    grep -F "video/x-actionlay-insv:*.insv" /usr/share/mime/globs2
    test -f /usr/share/licenses/actionlay-bin/LICENSE.txt
    # Same-version reinstall exercises pacman upgrade/removal and hook handling.
    pacman -U --noconfirm actionlay-bin-*.pkg.tar.zst
    pacman -R --noconfirm actionlay-bin
    test ! -e /usr/bin/actionlay
    test ! -e /usr/share/applications/org.ActionLay.ActionLay.desktop
    cp PKGBUILD .SRCINFO actionlay-camera-video.xml actionlay-bin-*.pkg.tar.zst /output/
  '
