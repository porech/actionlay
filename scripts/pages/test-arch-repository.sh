#!/usr/bin/env bash
# Exercise the signed repository through a real pacman client with a temporary key.
set -euo pipefail
PACKAGES="$(cd "${1:?package directory}" && pwd)"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT
export GNUPGHOME="$TEST_ROOT/gnupg"
mkdir -m 700 "$GNUPGHOME"
gpg --batch --pinentry-mode loopback --passphrase '' --quick-generate-key 'ActionLay CI repository <ci@example.invalid>' ed25519 sign 1d
GPG_FPR="$(gpg --list-secret-keys --with-colons | awk -F: '/^fpr:/ {print $10; exit}')"
export GPG_FPR
bash scripts/pages/build-arch-repository.sh "$PACKAGES" "$TEST_ROOT/repo"
docker run --rm --platform linux/amd64 -v "$TEST_ROOT/repo:/repo:ro" archlinux:base-devel bash -euxo pipefail -c '
  pacman --disable-sandbox -Syu --noconfirm
  pacman-key --init
  pacman-key --add /repo/key.asc
  fingerprint=$(gpg --show-keys --with-colons /repo/key.asc | awk -F: '\''/^fpr:/ {print $10; exit}'\'')
  pacman-key --lsign-key "$fingerprint"
  printf "\nDisableSandbox\n\n[actionlay]\nSigLevel = PackageRequired DatabaseRequired\nServer = file:///repo\n" >> /etc/pacman.conf
  pacman -Sy --noconfirm actionlay-bin
  pacman -Qo /usr/bin/actionlay
  actionlay-telemetry --version
  pacman -R --noconfirm actionlay-bin
'
# A corrupted package must not pass signature verification.
package=$(find "$TEST_ROOT/repo" -name '*.pkg.tar.zst' -print -quit)
printf 'tampered' >> "$package"
if gpg --verify "$package.sig" "$package"; then
  echo 'Corrupted package unexpectedly verified' >&2
  exit 1
fi
