# ActionLay

[![ci](https://github.com/porech/actionlay/actions/workflows/ci.yml/badge.svg)](https://github.com/porech/actionlay/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

**ActionLay** plays your action-camera videos with a live telemetry dashboard on
top: speed, altitude, maps, heart rate and more. You can also design the
dashboard visually and export the result. It is free, open source, and ships
as a single executable for Windows, macOS and Linux.

> **Status: early prototype (milestones M1 and M2 done).** ActionLay plays
> GoPro footage with hardware decoding and draws a live telemetry overlay
> (speed, altitude, gradient, distance, coordinates, date and time, GPS
> status) on top of it. The other dashboard widgets, the layout editor and
> the export are being built next; see the [roadmap](#roadmap). Expect rough
> edges.

## What works today

- Plays GoPro MP4 files (HEVC and H.264, including 4K, 10-bit and 100 fps
  footage) with **hardware decoding**: VideoToolbox on macOS, Direct3D 11 on
  Windows, VA-API on Linux. It falls back to software decoding automatically.
- **Audio in sync with video**: the audio drives the playback clock. If the
  audio device disappears (e.g. Bluetooth headphones), playback carries on.
- Correct colours, including the full-range footage GoPro cameras record.
- Frame-accurate seeking, frame-by-frame stepping, and playback speeds from
  0.25x to 4x.
- No installation and nothing else to download: FFmpeg is built into the
  executable.
- Reads GoPro telemetry (GPS, speed, altitude, accelerometer, gravity,
  orientation, camera temperature) and computes the same derived metrics as
  gopro-dashboard-overlay. `actionlay-telemetry dump VIDEO` prints them as
  CSV or JSON; `actionlay-telemetry info VIDEO` shows where data is missing.
  The tool ships in the same download as `actionlay`; run it from a terminal,
  or from source with `cargo run -p actionlay-telemetry-cli -- dump VIDEO`.
- **Live telemetry overlay**: open a GoPro video and a default dashboard
  appears over it, following the video as you play, pause and seek. Press
  `O` to show or hide it. Where the video has no GPS (or the signal is lost),
  values dim and then show `—` instead of stale numbers.
- **Buffered playback**: video, audio and GPMF share one progressive read.
  A separate I/O thread reads 1 MiB blocks and keeps a 32 MiB cache across
  seeks; compressed packets use a separate 32 MiB limit and read about
  3 seconds ahead. Playback starts after about 2 seconds are ready (or
  at EOF/the memory limit). During a source underrun both clocks wait and
  the transport shows Buffering. A seek starts buffering from its target;
  pause and opening another file cancel the pending playback action.
  Telemetry appears as packets arrive, without scanning the whole video.
  Unread ranges show empty values; the odometer waits for a complete prefix.
- **Your own layout**: the dashboard is a JSON file. Drag a `.ovl.json` file
  onto the window to use it; ActionLay remembers the last one. The default
  layout is in `crates/layout/layouts/default.ovl.json`, and a JSON Schema for
  editing it is in `crates/layout/schema`. The visual editor comes in M4.
- **Responsive telemetry presets**: Select Layout includes Default, Moto
  (braking/acceleration bars) and Training (heart-rate/power zones), inspired
  by the upstream dashboards. Widget sizes follow the video, and margins
  use relative distances. Appearance controls change accent, panel opacity
  and unit system across layouts and save immediately. Maps, charts and
  circular gauges are still part of the remaining M3 work.

## Roadmap

| Milestone | What you get |
|---|---|
| **M0** ✅ | Video player with hardware decoding and synced audio |
| **M1** ✅ | Telemetry from GoPro files (GPS, speed, altitude, accelerometer, …) |
| **M2** ✅ | Dashboard overlay drawn live on the video |
| M3 | All dashboard widgets: gauges, charts, compasses, moving and journey maps, plus a G-meter. Every widget is deeply customisable, with good defaults and a polished look when data is missing. Layouts from gopro-dashboard-overlay can be imported |
| M4 | Visual layout editor: add, move, resize and style widgets, with anchors that adapt to any resolution or aspect ratio. It warns you when a widget can't work with the data in your video, and layouts can be shared as files |
| M5 | Export: the final video, or a transparent overlay-only track (ProRes 4444 / PNG) for your video editor |
| M6 | GPX/FIT files from bike computers and watches, other cameras (DJI, Insta360, …), and GoPro chapters joined automatically |
| M7 | Polished releases for Windows, macOS and Linux |

The full design is in [docs/superpowers/specs](docs/superpowers/specs/).

## Download

There are no official releases yet. Until there are, you can grab the latest
development build:

1. Open the [latest successful CI run](https://github.com/porech/actionlay/actions/workflows/ci.yml?query=branch%3Amain+is%3Asuccess).
2. Download the artifact for your system (you need to be signed in to GitHub):
   - `actionlay-x86_64-pc-windows-msvc`: Windows 10/11, 64-bit
   - `actionlay-aarch64-apple-darwin`: macOS on Apple Silicon
   - `actionlay-x86_64-unknown-linux-gnu`: Linux, 64-bit
3. Unzip it and run `actionlay`.

The builds are not code-signed yet:

- **macOS**: the first time, right-click the file and choose *Open*, or allow
  it under *System Settings → Privacy & Security*. You may also need
  `chmod +x actionlay`.
- **Windows**: if SmartScreen warns about an unknown publisher, choose
  *More info → Run anyway*.

## Usage

Open a video by passing it on the command line:

```
actionlay GX010123.MP4
```

or choose **File → Open Video…** (⌘O on macOS, Ctrl+O on Windows/Linux).
When no video is open, clicking the welcome screen opens the system file
picker too. Dragging a file into the window still works.

On macOS, File is in the system menu bar; on Windows/Linux it is at the top
of the window. **File → Select Layout…** shows the included dashboards,
recent layouts and an option to open a `.ovl.json` file.
**File → Recent Videos** lists the last ten successfully opened videos;
**Clear Recent Videos** clears that list. History is saved immediately after
each action.
**File → Close Video** returns to the welcome screen. The layout stays selected.

| Key | Action |
|---|---|
| ⌘O / Ctrl+O | Open video |
| ⌘⇧O / Ctrl+Shift+O | Select layout |
| ⌘W / Ctrl+W | Close video |
| ⌘Q / Ctrl+Q | Quit |
| Space | Play / pause |
| → / ← | Next / previous frame |
| O | Show / hide the telemetry overlay |

The bar at the bottom also has the seek slider, the playback speed and some
playback statistics: the decoder in use, the dropped frames, the
audio/video offset and the time it takes to draw the overlay.

## Building from source

You need Rust (the version is pinned in `rust-toolchain.toml` and installed
automatically by rustup), Git, and the tools needed to build FFmpeg:
`nasm` and a C compiler. On Linux you also need the `libva` and ALSA
development headers.

```bash
git clone https://github.com/porech/actionlay.git
cd actionlay
./scripts/build-ffmpeg.sh            # builds a static FFmpeg into third_party/ (takes a few minutes)
source scripts/env.sh                # tells cargo where that FFmpeg is
cargo run --release -p actionlay-app -- path/to/video.mp4
```

On Windows, build FFmpeg from an MSYS2 shell that inherits the Visual Studio
environment. The exact steps are in [.github/workflows/ci.yml](.github/workflows/ci.yml).

To run the tests, first generate the synthetic sample videos. This needs an
`ffmpeg` command with libx264 and libx265:

```bash
./scripts/make-synthetic-samples.sh
./scripts/fetch-gopro-samples.sh     # public GoPro samples, ~33 MB
cargo test --workspace -- --test-threads=1

# Limited playback/seek diagnostics using the UI's player and telemetry decoder:
cargo run --release -p actionlay-app --example playback-check -- /path/to/video.mp4 240
```

## Contributing

Issues and pull requests are welcome. The project is young, so if you plan a
larger change, please open an issue first so we can agree on the approach.
Sample videos from cameras other than GoPro (with their telemetry) are
especially useful. Share only footage you are happy to publish, as videos
carry GPS positions.

## Credits

ActionLay stands on the shoulders of these open-source projects:

- **[gopro-dashboard-overlay](https://github.com/time4tea/gopro-dashboard-overlay)**
  by time4tea is the main inspiration for this project, and the source of its
  know-how on GoPro telemetry, metrics and dashboard widgets. Its layouts will
  be importable into ActionLay.
- **[Gyroflow](https://github.com/gyroflow/gyroflow)** is the architectural
  reference for a Rust desktop app that plays and processes action-camera
  footage.
- **[gpmf-parser](https://github.com/gopro/gpmf-parser)** by GoPro documents
  the GPMF telemetry format; its sample videos (Apache-2.0) are ActionLay's
  telemetry test files.
- **[telemetry-parser](https://github.com/AdrianEddy/telemetry-parser)** by
  AdrianEddy is the planned telemetry reader for DJI, Insta360 and other
  cameras.
- **[GeographicLib](https://geographiclib.sourceforge.io)** (Charles Karney),
  through [geographiclib-rs](https://github.com/georust/geographiclib-rs),
  computes distances and bearings exactly as gopro-dashboard-overlay does.
- **[FFmpeg](https://ffmpeg.org)** does the decoding, through the
  [ffmpeg-next](https://github.com/zmwangx/rust-ffmpeg) Rust bindings.
- **[tiny-skia](https://github.com/linebender/tiny-skia)** draws the overlay
  and **[cosmic-text](https://github.com/pop-os/cosmic-text)** shapes its text.
- **[Roboto](https://github.com/googlefonts/roboto)** (Apache-2.0) is the
  overlay font, and **[Tabler Icons](https://tabler.io/icons)** (MIT) provide
  its icons.
- **[egui / eframe](https://github.com/emilk/egui)** and
  **[wgpu](https://github.com/gfx-rs/wgpu)** power the user interface and the
  GPU rendering.
- **[cpal](https://github.com/RustAudio/cpal)** and
  **[ringbuf](https://github.com/agerasev/ringbuf)** handle audio output.
- **[OpenStreetMap](https://www.openstreetmap.org/copyright)** contributors
  will provide the map data for the map widgets.

## License

ActionLay is free software, released under the
[GNU General Public License v3.0 or later](LICENSE).

The binaries include FFmpeg, built with `--enable-gpl` and never with
`--enable-nonfree`, under the GNU GPL v2.0 or later. The exact FFmpeg
version and build options are in [scripts/](scripts/).
