#!/usr/bin/env bash
# CI has already validated and packaged all four native builds.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
: "${GITHUB_SHA:?}"
: "${GITHUB_REF:?}"
: "${GITHUB_REPOSITORY:?}"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
NOTES="$(mktemp /tmp/actionlay-release-notes.XXXXXX)"
trap 'rm -f "$NOTES"' EXIT
if [[ "$GITHUB_REF" == refs/tags/v* ]]; then
  TAG="${GITHUB_REF#refs/tags/}"
  cp docs/release-notes.md "$NOTES"
  if gh release view "$TAG" >/dev/null 2>&1; then
    echo "Stable release $TAG already exists; refusing to replace its assets" >&2
    exit 1
  fi
  # Upload into a draft, then make the complete set visible together.
  gh release create "$TAG" --verify-tag --draft --title "ActionLay ${TAG#v}" --notes-file "$NOTES"
  gh release upload "$TAG" dist/*
  gh release edit "$TAG" --draft=false --latest
elif [[ "$GITHUB_REF" == refs/heads/main ]]; then
  TAG=nightly
  cat > "$NOTES" <<NOTES
Development build from commit [$GITHUB_SHA](https://github.com/$GITHUB_REPOSITORY/commit/$GITHUB_SHA).

Updated after successful builds of main. This prerelease can contain unfinished changes.
For regular use, download the [latest stable release](https://github.com/$GITHUB_REPOSITORY/releases/latest).

macOS: universal DMG for Apple Silicon and Intel, macOS 12 or newer.
Windows: run actionlay-$VERSION-windows-x64-setup.exe, or use the portable ZIP.
Linux: use the signed APT/DNF repositories at https://porech.github.io/actionlay/, native DEB/RPM packages, or the portable tar.gz.
Builds are not signed by an identified publisher or Apple-notarized.
NOTES
  if gh release view "$TAG" >/dev/null 2>&1; then
    gh api "repos/$GITHUB_REPOSITORY/git/refs/tags/$TAG" --method PATCH -f sha="$GITHUB_SHA" -F force=true
    gh release upload "$TAG" dist/* --clobber
    # Versioned names change between builds. Keep this moving development
    # release consistent, including the package set used by Linux repositories.
    while IFS= read -r asset; do
      if [[ ! -f "dist/$asset" ]]; then
        gh release delete-asset "$TAG" "$asset" --yes
      fi
    done < <(gh release view "$TAG" --json assets --jq '.assets[].name')
    gh release edit "$TAG" --title "Development build" --notes-file "$NOTES" --prerelease --latest=false
  else
    gh release create "$TAG" --target "$GITHUB_SHA" --title "Development build" --notes-file "$NOTES" --prerelease --latest=false dist/*
  fi
else
  echo "No release is published for $GITHUB_REF"
fi
