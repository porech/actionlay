#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Git Bash runners may not have ripgrep; grep is part of every desktop CI shell.
if command -v rg >/dev/null 2>&1; then
  match=(rg)
else
  match=(grep -E)
fi
web_tree="$(cargo tree --locked -p actionlay-web --target wasm32-unknown-unknown -e normal)"
if echo "$web_tree" | "${match[@]}" 'ffmpeg|cpal|actionlay-media|fontconfig|winreg|objc2'; then
  echo 'Native dependency leaked into the browser backend.' >&2
  exit 1
fi
for native_target in aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu; do
  native_tree="$(cargo tree --locked -p actionlay-app --target "$native_target" -e normal,build)"
  if echo "$native_tree" | "${match[@]}" 'actionlay-web|wasm-bindgen|web-sys|js-sys'; then
    echo "Browser dependency leaked into the desktop backend: $native_target." >&2
    exit 1
  fi
done
