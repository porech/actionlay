#!/usr/bin/env bash
# Downloads the public GoPro sample videos used by the telemetry tests into
# samples/gopro (or $1). Source: github.com/gopro/gpmf-parser, Apache-2.0,
# pinned to one commit and checked by SHA-256. Files already present with
# the right checksum are kept.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/samples/gopro}"
COMMIT=9a7150632892c7356c91145c889016f07b0ed48d
BASE="https://raw.githubusercontent.com/gopro/gpmf-parser/$COMMIT/samples"
SAMPLES=(
  "hero5.mp4 04e45b2f41dff195b525fa18b7ed5517e5fc2d651127d1b57d1d931de306f915"
  "hero6.mp4 84aebc4e370ef9081f9015bf310d7a858d431258f6b0f2160d731c4308249c67"
  "hero7.mp4 3c593b8f08090e3ec246178c34737d11569330d75df8225e3ee08036c6a04d0b"
  "hero8.mp4 0e068f543ebf59bccb4228e7b5950a4f681753d2bbbf1c838ae60336bc75c1bd"
  "max-heromode.mp4 8e8fa98887f86119be1b886762b1080a92afb6cc146db719a238bdcba908277b"
)

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

mkdir -p "$OUT"
for entry in "${SAMPLES[@]}"; do
  read -r name sum <<<"$entry"
  dest="$OUT/$name"
  if [ -f "$dest" ] && [ "$(sha256 "$dest")" = "$sum" ]; then
    echo "ok   $name (already present)"
    continue
  fi
  curl -fsSL --retry 3 -o "$dest.part" "$BASE/$name"
  got="$(sha256 "$dest.part")"
  if [ "$got" != "$sum" ]; then
    rm -f "$dest.part"
    echo "checksum mismatch for $name: got $got, want $sum" >&2
    exit 1
  fi
  mv "$dest.part" "$dest"
  echo "ok   $name (downloaded)"
done
