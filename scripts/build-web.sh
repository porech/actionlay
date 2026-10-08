#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# The CLI must match Cargo.lock. Install it outside any project environment.
command -v wasm-bindgen >/dev/null || { echo 'Install wasm-bindgen-cli 0.2.129, then retry.' >&2; exit 1; }
rustup target add wasm32-unknown-unknown
cargo build --locked --release -p actionlay-web --target wasm32-unknown-unknown
mkdir -p web/public/pkg
wasm-bindgen target/wasm32-unknown-unknown/release/actionlay_web.wasm --target web --out-dir web/public/pkg --out-name actionlay_web
npm --prefix web ci
npm --prefix web run build
