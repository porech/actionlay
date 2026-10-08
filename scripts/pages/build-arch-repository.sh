#!/usr/bin/env bash
# Sign packages and pacman's database with the existing repository key.
set -euo pipefail
PACKAGES="$(cd "${1:?package directory}" && pwd)"
mkdir -p "${2:?repository directory}"
REPO="$(cd "$2" && pwd)"
: "${GPG_FPR:?signing fingerprint required}"
cp "$PACKAGES"/*.pkg.tar.zst "$REPO/"
for package in "$REPO"/*.pkg.tar.zst; do
  gpg --batch --yes --default-key "$GPG_FPR" --detach-sign "$package"
done
# Only public packages/signatures enter the container; the private key stays outside.
docker run --rm --platform linux/amd64 -v "$REPO:/repo" archlinux:base-devel \
  bash -euo pipefail -c 'cd /repo; repo-add actionlay.db.tar.gz ./*.pkg.tar.zst'
for index in db files; do
  gpg --batch --yes --default-key "$GPG_FPR" --detach-sign "$REPO/actionlay.$index.tar.gz"
  # Pages artifacts cannot contain symlinks.
  rm -f "$REPO/actionlay.$index"
  cp "$REPO/actionlay.$index.tar.gz" "$REPO/actionlay.$index"
  cp "$REPO/actionlay.$index.tar.gz.sig" "$REPO/actionlay.$index.sig"
  gpg --verify "$REPO/actionlay.$index.sig" "$REPO/actionlay.$index"
done
gpg --armor --export "$GPG_FPR" > "$REPO/key.asc"
