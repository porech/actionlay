# Building and contributing

This guide covers the desktop application, telemetry CLI and browser application.
The desktop uses native media and operating-system services; the browser shares
Rust telemetry, layout and rendering code through WebAssembly and supplies its own
media and storage adapters. Start with the build for the platform you want to work
on, then use the architecture and verification sections to scope your changes.

- [Desktop setup](#desktop-setup)
- [Browser setup](#browser-setup)
- [Architecture and source map](#architecture-and-source-map)
- [Development workflow](#development-workflow)
- [Tests and regression checks](#tests-and-regression-checks)
- [Playback diagnostics](#playback-diagnostics)
- [Troubleshooting](#troubleshooting)
- [Contributing changes](#contributing-changes)
- [CI, releases and deployment](#ci-releases-and-deployment)

## Desktop setup

### Common requirements

Install Git and Rust through rustup. The checkout's
[`rust-toolchain.toml`](../rust-toolchain.toml) pins Rust, rustfmt and Clippy;
rustup installs that toolchain when you run Rust commands in the repository.
Use the committed `Cargo.lock` to keep dependency versions reproducible.

The native build also needs a C/C++ compiler, Make, CMake, pkg-config and NASM.
FFmpeg bindings need libclang. The build scripts compile pinned static FFmpeg,
x264 and x265 libraries into `third_party/`; a system FFmpeg library installation
is not a substitute for this build. The system `ffmpeg` executable is separately
needed to generate test videos, and `ffprobe` to inspect browser export fixtures.

### macOS

Install the Xcode Command Line Tools and the build tools, for example:

```sh
xcode-select --install
brew install git make cmake pkg-config nasm ffmpeg
```

The current CI builds Apple Silicon and Intel separately, with a macOS 12
minimum deployment target. A local build targets the architecture of your Rust
host; producing a universal application is a separate packaging step.

### Linux

For an Ubuntu/Debian development machine:

```sh
sudo apt-get update
sudo apt-get install -y build-essential git make cmake pkg-config nasm \
  clang libclang-dev libva-dev libasound2-dev ffmpeg
```

Use an equivalent package set on other distributions. Running the GUI requires
an audio device and a graphics backend supported by wgpu. The app enables both
Wayland and X11. Distributed Linux binaries use the Ubuntu 22.04/glibc 2.35
baseline; this is not a musl/Alpine build.

### Windows

Use the `x86_64-pc-windows-msvc` Rust toolchain, Visual Studio 2022 or its Build
Tools with the C++ workload and Windows SDK, LLVM/libclang, CMake and MSYS2.
In an MSYS2 UCRT64 shell, install the shell build tools:

```sh
pacman -S --needed make nasm diffutils git pkgconf
```

Start that shell with the x64 Visual Studio developer environment inherited so
MSVC's compiler and linker are available, and put CMake on PATH. Set
`LIBCLANG_PATH` to LLVM's library directory if bindgen cannot find libclang.
MSYS2/Git Bash can also provide a `link.exe` from coreutils: Cargo must use the
**MSVC** linker. Set `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER` to its full path
if necessary. The repository's `.cargo/config.toml` selects the static CRT.

The MSVC environment, MSYS2, libclang and linker steps in
[the CI workflow](../.github/workflows/ci.yml) are the reference for the Windows
build. Run the Makefile commands below from the configured Bash environment.
The browser-only build does not need MSVC or native FFmpeg.

### Build, run and use Cargo directly

From a clone or your fork:

```sh
git clone https://github.com/porech/actionlay.git
cd actionlay
make run
make run ARGS="path/to/video.mp4"
```

`make run` builds native dependencies if needed, then launches a **release**
build. Release mode is useful for playback and rendering performance work;
debug builds are available for stepping through code. The first native build
can take several minutes. Later builds reuse `third_party/` and Cargo's caches.
The bundled media configuration and revisions are in
[`build-ffmpeg.sh`](../scripts/build-ffmpeg.sh),
[`ffmpeg-version.env`](../scripts/ffmpeg-version.env) and
[`build-encoders.sh`](../scripts/build-encoders.sh).

| Command | Result |
|---|---|
| `make build` | Release binaries in `target/release/`, including `actionlay`, `actionlay-telemetry` and `decode-bench` |
| `make ffmpeg` | Build/reuse the host's static media libraries |
| `make fmt` | Format Rust source |
| `make check` | Check Rust formatting and run Clippy |
| `make samples` | Generate synthetic clips and fetch public GoPro clips |
| `make test` | Run workspace tests serially |
| `make help` | Show available Makefile targets |

The Makefile supplies `FFMPEG_DIR=third_party/ffmpeg/<Rust-host-target>`. To run Cargo
directly in Bash, prepare those libraries and export their location first:

```sh
bash scripts/build-ffmpeg.sh
source scripts/env.sh
cargo run --locked --bin actionlay -- path/to/video.mp4
cargo run --locked --release --bin actionlay-telemetry -- info path/to/video.mp4
cargo run --locked --release --bin actionlay-telemetry -- dump path/to/video.mp4
```

`source scripts/env.sh` is a Bash helper. In other shells, set `FFMPEG_DIR` to
the absolute path of the corresponding `third_party/ffmpeg/<target>` directory.
The export lasts for that shell session. Build outputs are executables, not the
installed app bundle, Windows installer or Linux packages; see
[release packaging](releasing.md) for those.

## Browser setup

### First build and local server

Install Rust/rustup, Node **24** and npm. The browser build needs the
`wasm32-unknown-unknown` Rust target and a bindings generator matching the
`wasm-bindgen` version pinned in `crates/web/Cargo.toml` and `Cargo.lock`.
It does not use native FFmpeg or a processing server.

```sh
cargo install wasm-bindgen-cli --version 0.2.129 --locked
bash scripts/build-web.sh
npm --prefix web run dev
```

The build script installs the WASM target through rustup, compiles the Rust
backend in release mode, generates bindings into `web/public/pkg/`, installs
JavaScript dependencies with `npm ci`, and builds `web/dist/`.
The Vite server normally opens at <http://127.0.0.1:5173>; check the URL it prints
if that port is occupied. To require that port explicitly:

```sh
npm --prefix web run dev -- --port 5173 --strictPort
```

Serve the application over localhost or HTTPS so browser media/filesystem APIs
are available. Opening `index.html` through `file://` is not the development
workflow. Codec support still depends on the browser and device.

### Iterating on JavaScript and Rust

Vite reloads shell JavaScript, CSS and HTML changes. Changes in shared Rust or
`crates/web` need a new WASM binary and bindings, followed by a browser reload:

```sh
cargo build --locked --release -p actionlay-web --target wasm32-unknown-unknown
wasm-bindgen target/wasm32-unknown-unknown/release/actionlay_web.wasm \
  --target web --out-dir web/public/pkg --out-name actionlay_web
```

When dependencies or build configuration change, use `bash scripts/build-web.sh`
again. `npm --prefix web run build` only builds the JavaScript/static bundle; it
does **not** regenerate Rust/WASM. Browser integration tests serve `web/dist/`,
so rebuild that bundle after development changes before running them.

Vite uses relative URLs, allowing the static bundle to be hosted under a
subdirectory. There is no requirement for SharedArrayBuffer or cross-origin
isolation. Format coverage, storage limits, map-provider CORS requirements and
browser export limitations are documented in [the browser guide](web.md).

## Architecture and source map

### Crates and platform boundaries

| Location | Responsibility |
|---|---|
| `crates/app` (`actionlay-app`) | Desktop egui/eframe interface, wgpu video compositing, editor, preferences, OS integration and export orchestration; provides the `actionlay` binary |
| `crates/media` (`actionlay-media`) | Native FFmpeg probing, demuxing/decoding, audio output, playback clocks/queues, chapters, indexed GPMF reads and video encoding |
| `crates/telemetry` (`actionlay-telemetry`) | GPMF parsing, telemetry sampling, derived metrics, GPS lock/filtering, external activities and native camera adapters; does not depend on FFmpeg |
| `crates/layout` (`actionlay-layout`) | Layout model/schema, geometry, validation, presets, XML import and portable packages/assets |
| `crates/render` (`actionlay-render`) | Shared CPU overlay renderer using tiny-skia, text/font handling, widgets, history, charts and maps |
| `crates/maps` (`actionlay-maps`) | Shared provider/privacy/coordinate model, native tile worker/disk cache, and asynchronous browser tile adapter |
| `crates/telemetry-cli` | `actionlay-telemetry` inspection and CSV/JSON commands |
| `crates/web` (`actionlay-web`) | WASM API for shared telemetry/render/layout operations and the shared visual editor |
| `web/src` | Browser shell, native video element, worker protocol, indexed metadata readers, storage, file drop/pickers and WebCodecs export |
| `scripts`, `packaging`, `.github/workflows` | Dependency builds, fixtures, artwork, installers, release assets and CI/Pages deployment |

The workspace's default members are desktop crates. `actionlay-web` is a
workspace member but its backend dependencies are restricted to WASM. Desktop
never depends on it. Native camera parsing, OS font discovery, filesystem/UI
services and native media remain on native targets; the browser supplies its own
clock, network and storage implementations where needed.

The web crate includes the desktop editor and i18n source modules by path.
Changes there must remain compilable on both targets. Use target-specific
implementations and Cargo dependencies for platform services, rather than
pulling a browser or native runtime into a shared algorithm.
[`check-backend-isolation.sh`](../scripts/check-backend-isolation.sh) checks the
WASM dependency tree and all four desktop trees, including build dependencies
on the desktop side. It does not replace runtime tests.

### Desktop playback, telemetry and rendering

The native player demuxes compressed input into bounded caches/queues and decodes
video and audio off the UI thread. Hardware video decoding has software fallback.
At normal speed with audio, the audio clock drives playback; a scaled system
clock handles other cases. Buffering and seek generations prevent stale decoded
frames from being presented after the timeline changes. Keep blocking reads,
decoding, expensive materialization and logging out of the audio callback.

GPMF packets encountered during playback feed progressive telemetry. A separate
indexed reader recovers historical metadata as declared by the shared layout
requirements: full sources for journey charts and fitted/full-route maps, chart
windows, and prefixes for cumulative metrics, filtered compasses and G-meter
calibration/peaks. Each historical widget has a loading percentage based on its
own interval; successfully read intervals with missing values are not unread data.
Its work is separate from the playback seek cursor. Partial telemetry
records loaded intervals so interpolation does not bridge unread gaps. Full
track completion is validated before switching a map to fitted route zoom.
Start in [`player.rs`](../crates/media/src/player.rs),
[`telemetry_load.rs`](../crates/app/src/telemetry_load.rs) and
[`gpmf.rs`](../crates/media/src/gpmf.rs) for this flow.

The overlay worker renders for the time of the displayed video frame, keeping
only the latest request and discarding obsolete layout/telemetry results.
The shared renderer produces **premultiplied RGBA**. The desktop composites that
over the video with wgpu; the browser converts it for Canvas ImageData. Changes
to alpha handling, fonts, units, scaling or map drawing can affect both preview
and export. Loading spinners belong to the interface, not exported pixels.

### Browser playback and export

The browser plays the user's local file in an HTML video element; JavaScript
uses its presentation time to request overlays. A Web Worker hosts the WASM
telemetry/renderer and media export. MP4Box reads the MP4 index and only the GPMF
byte ranges. Foreground metadata reads follow playback/seek windows; an
independent history reader supplies the required intervals. Both readers share
packet reads and publish progressive telemetry. Seeking does not restart the full-route
reader. Export acquires a validated complete source before rendering when any
visible widget requires it; otherwise it acquires each frame's required history
as export advances. Loading badges are UI elements and never enter exported pixels.

Mediabunny supplies browser demuxing, WebCodecs decoding/encoding and MP4 muxing.
Exports await backpressure instead of retaining all decoded frames. File-system
export commits only on success and aborts on cancellation; the fallback is a
bounded in-memory download. Native export has different format coverage and
cancellation behavior, described in the README and [browser guide](web.md).
Do not assume a feature in a shared crate is wired into both frontends: external
activities, native camera formats and joined chapters currently have desktop
playback support but are not connected in the browser shell.

### Layouts, preferences and localization

A `.actionlay-layout` package contains JSON plus declared fonts/images. Shared
package I/O works with seekable streams; desktop filesystem saves retain their
synchronized temporary-file replacement. Unknown fields and supported imported
layout data must survive round trips. Treat imported paths and sizes as untrusted
input and retain the package validation limits.

Desktop keeps preferences and recent layout locations through its native
preference layer. Browser preferences and up to ten recent **complete packages**
use localStorage, while videos must be selected again after reload. Storage quota
errors preserve the last successfully stored state; file downloads provide an
independent way to retain a layout.

Desktop translation catalogues live in `crates/app/locales`; browser-specific
messages live in `web/locales` and common labels reuse the desktop catalogues.
The browser defaults to the browser locale and persists an explicit choice.
Interface language and measurement units are independent. English message keys,
numbered placeholders, RTL shaping and font subsets are explained in
[the localization guide](../crates/app/locales/README.md).

## Development workflow

Find the layer that owns a behavior before changing it. A widget or telemetry
calculation usually belongs in shared Rust; a browser file picker belongs in the
shell; native decoder scheduling belongs in the media layer. Inspect the current
source and tests as well as design notes: historical milestone reports in `docs/`
explain decisions but may describe an earlier implementation.

For focused native work after preparing `FFMPEG_DIR`:

```sh
source scripts/env.sh
cargo test --locked -p actionlay-layout
cargo test --locked -p actionlay-render
cargo test --locked -p actionlay-telemetry
cargo test --locked -p actionlay-app -- --test-threads=1
cargo test --locked -p actionlay-media --test player -- --test-threads=1
```

Changing a shared module requires checking both desktop and WASM compilation.
Changing a widget may also require schema validation, editor controls, XML import
mapping, hit bounds, unit handling, translations and golden images. See
[the layout/widget reference](../crates/layout/layouts/README.md) and the tests
beside the relevant code. Changes to timing/queues should include a reproduction
and exercise seek, buffering, cancellation and repeated recovery, as applicable.

Rust dependencies belong in Cargo manifests and `Cargo.lock`; JavaScript
versions belong in `web/package.json` and `web/package-lock.json`. Generated WASM,
`web/dist`, `web/node_modules`, test output, Cargo output, fetched media and
`third_party` build trees are ignored and should not be committed. Intentionally
versioned fixtures, fonts, artwork and render goldens are exceptions with their
own provenance and generation instructions.

Python is only needed for selected fixture, artwork and packaging helpers, not
to run the application. Use a project's existing venv only if it already has the
needed dependencies. Standard-library-only scripts can use system Python;
otherwise create a temporary venv under `/tmp` for the helper. Do not install
helper packages into system Python or add them to a project's venv for tool use.

## Tests and regression checks

### Native suite and sample data

Install a system `ffmpeg` with libx264/libx265, then prepare public fixtures:

```sh
make samples
make check
ACTIONLAY_REQUIRE_GOPRO_SAMPLES=1 make test
```

`make test` serializes workspace tests because player tests open audio devices
and measure timing. Running them concurrently can produce misleading failures.
The explicit GoPro requirement makes missing public samples a failure rather
than a silently skipped reference check. Additional native-camera and external
activity fixture downloaders are in `scripts/fetch-camera-samples.py` and
`scripts/fetch-external-samples.sh`; inspect their README/source and relevant tests
when working on those formats. Some downloaded samples are for local validation
only and must not be committed or redistributed; follow the downloader notices.
Camera/firmware coverage varies.

Use synthetic or publicly licensed media for committed tests. Never add personal
footage, coordinates, private-file derivatives or secrets to fixtures, goldens,
logs or release artifacts. Public GoPro provenance and reference-value generation
are documented in `samples/gopro/README.md` and
[the telemetry reference guide](../crates/telemetry/tests/reference/README.md).

Renderer tests compare against existing golden images and test stable pixel
regions separately. On a failure, inspect `target/golden-diff/` and the test's
actual/diff images. Regenerate references only for an intended visual change,
reviewing and including the new images in the same contribution:

```sh
source scripts/env.sh
ACTIONLAY_UPDATE_GOLDENS=1 cargo test --locked -p actionlay-render --test golden
```

### Browser suite and shared-code checks

After generating WASM and building the production bundle:

```sh
bash scripts/check-backend-isolation.sh
cargo clippy --locked -p actionlay-web --target wasm32-unknown-unknown -- -D warnings
npm --prefix web test
cd web
npx playwright install chromium
npm run test:browser
```

On Linux, Playwright may need OS packages: CI uses
`npx playwright install --with-deps chromium`. Browser tests require system
`ffmpeg` and `ffprobe`, generate synthetic media and serve the production bundle
under `/web/` on port 4173. They exercise persistence, packaged assets, editor,
progressive/seek metadata, playback controls, file drop, export and cancellation.
An existing development server on 5173 is not their test server.

For a shared-code change, run the applicable native tests, the WASM build and
browser checks. For a frontend-only change, focus on that frontend's behavior.
Use tests that expose a regression or contract rather than restating a helper's
implementation. Keep the existing golden references during a platform port.

Before a release or a substantial media/UI change, also try the desktop app:
play video with audio, seek and cross chapter boundaries, inspect full-route
maps, edit/undo and reopen packaged layouts with assets, and export a trimmed
video and a solid-background overlay. For web, check a browser/device that
represents the intended codec and filesystem support, fullscreen idle controls,
reload persistence and cancellation. Automated tests on one OS cannot establish
hardware/audio behavior on every other platform.

## Playback diagnostics

For playback diagnostics, enable **Show diagnostic data** in **Settings → Advanced**.
The same window offers **Use software video decoding**, applied when a video is
next opened; leave it disabled to prefer hardware with software fallback.
`ACTIONLAY_NO_HW=1` still forces software decoding for command-line comparisons.
Set `RUST_LOG=actionlay_media=debug` and capture stderr.
The player reports a snapshot once per second: monotonic elapsed time, Unix wall
time in milliseconds, generation, audio/system clocks, decoded/presented video
PTS, queue sizes, buffering state, backend, and audio device format. The audio
tuple contains clock seconds, queued/consumed stereo frames, sample rate, then
device latency in microseconds, cumulative underrun callback count, and cumulative
silence frames inserted because samples were missing. Silence caused by user
volume zero or internal suspension during buffering is not counted as starvation.
Slow cache-miss reads (at least 100 ms) and buffering transitions are also logged.
`video_us` contains cumulative microseconds spent in codec calls, hardware-frame
download, and NV12 conversion respectively; compare successive snapshots with
`decoded` to estimate their cost per frame. `skipped_outputs` counts decoded
frames omitted before download/conversion because they would miss the playback
deadline, including the recent measured materialization cost while the audio
clock continues. This preserves codec reference frames and does not discard
compressed packets. It is separate from the presentation queue's `dropped` count; rejected
packets and codec errors produce explicit warnings.
No logging occurs in the audio data callback. A negative A/V diagnostic means the
last presented video PTS trails the audio clock; it alone does not establish
audible-content synchronization. Compare a captured output with the source audio
when investigating gaps or drift.

Use the existing playback diagnostic example when narrowing a reproduction:

```sh
source scripts/env.sh
cargo run --locked --release -p actionlay-app --example playback-check -- /path/to/video.mp4 240
cargo run --locked --release -p actionlay-app --example playback-check -- /path/to/video.mp4 240 45
```

The optional second argument is a **seek target in seconds**, not a run duration.
The example exercises initial playback, that seek, and a return to the start.
The optional third is the poll interval in milliseconds, allowing a slow UI to be
simulated. The example
uses the app's player and telemetry decoder; it is a limited diagnostic rather
than an automated assertion of audible A/V synchronization. Sanitize logs before
sharing them, since file paths and telemetry can identify private recordings.

For web issues, use the browser console and worker errors, and reproduce against
a freshly built `web/dist/` as well as Vite. Describe the browser/version, codec,
source dimensions, chosen export mode and whether direct file saving or the
download fallback was used.

## Troubleshooting

| Symptom | Checks |
|---|---|
| FFmpeg headers/libraries not found | Run `make ffmpeg`, verify the host target directory, and set `FFMPEG_DIR` when bypassing Make |
| Bindgen cannot find libclang | Install the platform's libclang/LLVM package and check `LIBCLANG_PATH`, particularly on Windows |
| Windows link errors or wrong `link.exe` | Use the x64 MSVC environment and explicitly select MSVC's linker; keep the static CRT configuration |
| Browser says the WASM module is missing | Run `scripts/build-web.sh`; Vite alone does not generate `web/public/pkg` |
| Rust edits do not appear in the browser | Regenerate WASM/bindings and reload; rebuild `web/dist` before production/browser tests |
| Bindings fail to load after a dependency change | Match the installed wasm-bindgen CLI exactly to the locked Rust crate version |
| Video/encoder unsupported in the browser | Check that browser/device's codec support and try H.264; native and web coverage differ |
| Maps fail only in the browser | Check provider CORS, HTTPS/mixed-content restrictions, network errors and map-download preferences |
| Recent layouts cannot be stored | Check localStorage availability/quota; download packages before changing saved data |
| Playback tests fail on a headless machine | Inspect audio-device/backend and timing diagnostics; test in an environment matching the reproduction |
| Build reports no space left | Check disk space; Rust/native build artifacts can be large. Cargo can clean generated outputs; retain source files and fixture provenance |

When investigating a platform issue, report whether the same input/layout fails
on the other frontend. Preserve useful failure output; a clean rebuild is a
cache diagnosis, not evidence that a runtime bug is fixed.

## Contributing changes

Open an issue or describe the problem in a pull request with a concrete trigger
and expected result. A useful report identifies the commit or release, OS and
architecture, browser if relevant, input format/camera, and steps to reproduce.
For performance/timing bugs include the relevant diagnostics and a comparison
using the same input. Supply a small public or synthetic reproduction where
possible, along with its license/provenance.

Work on a topic branch (and fork for contributors without repository access).
Keep unrelated refactors and dependency updates out of a focused fix. Explain
which layer changed, why, the observable behavior, and which checks you ran.
Include platform limitations and any checks you could not run. A shared-code PR
should account for both desktop and browser builds, including their dependency
boundaries. Translation contributions should follow the catalogue/font guidance;
new formats or widgets should document their coverage and limitations.

Code contributions are part of a GPL-3.0-or-later project. Third-party code,
fixtures, fonts and artwork need compatible licensing and attribution; see
[credits](credits.md) and the source assets' README/license files. Review generated
assets as well as the script/source that produced them. Avoid introducing runtime
processing services or platform dependencies without discussing the architectural
impact in the contribution.

## CI, releases and deployment

[CI](../.github/workflows/ci.yml) builds/tests desktop on macOS ARM64, macOS Intel,
Windows x64 and Linux x64. It also builds WASM, tests the browser production bundle
and enforces backend isolation. Desktop packaging tests cover Windows install/
upgrade/uninstall, Linux packages and macOS packaging checks. Contributor builds
and pull requests need no release signing keys or Pages credentials.

Successful `main` builds publish the desktop `nightly` development release.
Stable `v*` tags must match the workspace version; they publish immutable release
assets, including the versioned browser bundle. The Pages job reconstructs signed
Linux repositories and installs the **latest stable browser release bundle**.
Main/nightly runs reuse that web bundle: they test web changes but do not publish
an untagged browser build. This separation lets web work be validated on `main`
without updating the hosted stable app.

Maintainer versioning, signing, installers and packaging commands are in
[the release guide](releasing.md). Browser deployment details and current feature
coverage are in [the browser guide](web.md). Keep these guides aligned when
changing the build, platform boundaries, persistent data formats or release flow.
