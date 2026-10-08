#!/usr/bin/env bash
# Push only a validated recipe. AUR account/key setup is documented in packaging/aur.
set -euo pipefail
RECIPE="$(cd "${1:?validated recipe directory required}" && pwd)"
: "${AUR_GIT_NAME:?AUR commit author required}"
: "${AUR_GIT_EMAIL:?AUR commit email required}"
test -s "$RECIPE/.SRCINFO"
VERSION="$(sed -n 's/^pkgver=//p' "$RECIPE/PKGBUILD")"
# 1.4.2 shipped before AUR support. The first submission is a subsequent release.
python3 - "$VERSION" <<'PY'
import re
import sys
v = sys.argv[1]
if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', v) or tuple(map(int, v.split('.'))) <= (1, 4, 2):
    raise SystemExit('AUR publication starts after 1.4.2')
PY
WORK="$(mktemp -d /tmp/actionlay-aur-publish.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT
git clone ssh://aur@aur.archlinux.org/actionlay-bin.git "$WORK/repo"
for file in PKGBUILD .SRCINFO actionlay-camera-video.xml; do cp "$RECIPE/$file" "$WORK/repo/$file"; done
cd "$WORK/repo"
git config user.name "$AUR_GIT_NAME"
git config user.email "$AUR_GIT_EMAIL"
git add PKGBUILD .SRCINFO actionlay-camera-video.xml
if git diff --cached --quiet; then exit 0; fi
git commit -m "Update actionlay-bin to $VERSION"
git push origin HEAD:master
