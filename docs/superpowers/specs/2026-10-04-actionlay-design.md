# ActionLay — Design

- **Date**: 2026-10-04
- **Status**: approved (2026-10-04)
- **Name**: ActionLay (provisional; available on GitHub and crates.io as of this date)
- **License**: GPL-3.0-or-later

## 0. Goal and success criteria

**Open source, cross-platform** desktop application (Windows, macOS, Linux) to:

1. **play back** action camera videos with a telemetry dashboard overlaid and synchronized in real time;
2. **create and edit** the dashboard in a visual editor (add, position, resize, configure elements);
3. **export** the video with the overlay, or just the transparent overlay.

It is inspired by [gopro-dashboard-overlay](https://github.com/time4tea/gopro-dashboard-overlay) (time4tea, GPL-3.0), which is credited as a source of inspiration and know-how; its layouts are imported and converted to the ActionLay format. There is no direct compatibility with the original XML format.

**Constraints**
- Written in **Rust**.
- Distributed as a **single executable** per platform, with no dependencies to install (exceptions: system GPU drivers; on macOS a `.app` bundle).

**Success criteria**
- Every layout of the original, once imported, is rendered **almost identically** to the original (measured with reference images, §8).
- A GoPro 4K/H.265 video (and the 1920×1440 sample at 100 fps) plays **smoothly** with the overlay on a recent laptop, on macOS and Windows.
- Preview and export are **identical** (same render engine).

## 1. v1 scope

### Included
- Player with real-time overlay, precise seek, frame step forward/back, variable speed.
- Full visual editor (§5).
- **All widget types** of the original (§4.3) — list taken from the component registry in `layout_xml.py`, not only from the documentation.
- Data sources: GoPro GPMF; external GPX and FIT synchronized by time of day; other cameras (DJI, Insta360, …) via telemetry-parser.
- GoPro chapters recognized and merged automatically; each file remains openable on its own.
- Export: final video (H.264/H.265) and overlay only (ProRes 4444 / PNG sequence with alpha); export of a range (in/out points).
- Command line for export (same crates as the app).
- Privacy zones (as in the original: map widgets do not draw points inside the zones).
- Remembering preferences and per-video state (§6); registration in "Open with" on Windows (§6.3).

### Excluded from v1 (explicit choices)
- Percentage-based positioning in layouts (anchors + uniform scale are enough).
- Audio at speed ≠ 1x (it is muted).
- Multi-clip timeline / editing.
- Export queue, advanced output bitrate/resolution selection.
- Code signing (macOS/Windows) and automatic updates (only a new-version notice).
- 360° dual-stream video (GoPro MAX `.360`).
- Reading/writing the original XML format beyond the one-time import.

## 2. Architecture

Cargo workspace with single-responsibility crates:

| Crate | Responsibility | Depends on |
|---|---|---|
| `telemetry` | Reading GPMF/DJI/Insta360 (telemetry-parser), GPX, FIT. Unified time series with interpolation and derived metrics. Alignment to video time; merging of external files by UTC time with adjustable offset; file-time ↔ real-time conversion for timelapse/TimeWarp. Privacy zones. | — |
| `layout` | Layout model (tree of nodes), widget schema, JSON (de)serialization, validation, importer for the original XML. | — |
| `maps` | Tile providers, download, disk cache, prefetch of the route area. | network (rustls) |
| `render` | Pure function: (layout, instant t, telemetry, size, scale) → premultiplied RGBA image. Also computes the bounding boxes of each node for the editor. Single engine for preview and export. | `layout`, `telemetry`, `maps` |
| `media` | Static ffmpeg: file and chapter opening, HW decoding, audio, export encoding. | ffmpeg |
| `app` | egui + wgpu UI: player, editor, export, preferences. Includes the export CLI. | all |

**Playback flow**: player clock → t → `telemetry` (values at t) → `render` (overlay) → GPU texture → compositing in the shader over the video frame.

**Export flow**: for each decoded frame → `render` of the overlay at its timestamp → compositing (or overlay only) → encoder.

## 3. Player and synchronization

**Threads**
- *Video decoding*: ffmpeg demux + HW decoding (VideoToolbox / D3D11VA / VA-API), multi-threaded software fallback with a warning. Short queue (~8 frames). Upload as YUV textures (NV12, P010 for 10 bit); conversion to RGB in the shader with a color matrix (BT.709/BT.2020) and correct ranges (e.g. `yuvj420p` = full range).
- *Audio*: AAC decoding → `cpal`. **Audio is the master clock**. Without audio or at speed ≠ 1x: system clock, audio muted.
- *Presentation*: at each vsync the frame with pts ≤ the latest clock value is shown; late frames are dropped (e.g. 100 fps on 60 Hz).
- *Overlay render*: thread separate from the UI, ~25 Hz during playback, immediate when paused/after a seek; double buffer. In preview it renders at display resolution (scaling the layout), in export at full resolution.

**Time**
- Everything is indexed on **file time** (pts). GPMF telemetry is aligned to the video pts.
- **Chapters**: virtual timeline concatenating the files (pts offsets); telemetry is concatenated in the same way. Opening `GX01xxxx` loads the following chapters; opening an intermediate chapter, the file works on its own and the user is offered to load the sequence from the first one.
- **Seek**: while dragging, keyframes only; on release, frame-accurate seek (decode from the previous keyframe, discarding intermediate frames). Frame ±1.

**Errors**: without HW decoding → software + warning; without telemetry → normal video, widgets show "no data"; corrupted files are read as far as possible.

## 4. Layout and render

### 4.1 Layout format (`*.ovl.json`)

Versioned JSON with a published JSON Schema.

- **Units**: 1 unit = 1/1080 of the video height. Values of the original's 1080p layouts transfer 1:1.
- **Header**: `version`, `name`, `design_aspect` (e.g. `16:9`), default unit system (metric/imperial), default font, base colors.
- **Node**: `id`, `name`, `type`, `anchor` (9 values: `top-left` … `bottom-right`), `offset` [x, y] from the anchor, the widget's own size, `opacity`, `visible`, specific parameters.
- **Groups**: with an explicit size, children can anchor inside the group; without a size, children are positioned relative to the group's origin (like the original `composite`s).
- **Data and text**: `metric`, `units`, `format` with its own syntax (`"{value:.0}"`, `"{unit}"`, dates with `strftime`).

Example:
```json
{ "id": "speed-main", "type": "metric", "anchor": "bottom-left", "offset": [16, -120],
  "metric": "speed", "units": "kmh", "format": "{value:.0}", "size": 160, "color": "#ffffff" }
```

### 4.2 Adapting to the resolution
- A single scale factor for all measurements. **`height` mode (default)**: `H / 1080`. **`fit` mode**: `min(H / 1080, W / (1080 × design_aspect))`, for vertical videos or videos narrower than the layout. The mode is chosen in the project.
- Positions follow the anchor (a bottom-right group stays bottom-right on 4:3, 16:9, 4K).
- The editor warns if any element falls outside the frame.

### 4.3 Widgets

Each widget type declares a **parameter schema** (type, default, range) that generates the properties panel, the validation, and the defaults of new widgets. Parameter types: choice from a list, number (range/step), RGBA color, font+size, boolean, text/format, metric (filtered by compatibility), image/icon.

Widget types to support in v1 (registry of the original's `layout_xml.py`):

- **Containers**: `composite`/`translate` (group), `frame` (group with background, border, radius, opacity, fade).
- **Text and data**: `text`, `metric`, `metric_unit`, `datetime`, `icon`, `gps_lock_icon`.
- **Maps**: `moving_map`, `journey_map`, `moving_journey_map`, `circuit_map`, `cairo_circuit_map`.
- **Charts**: `chart`, `gradient_chart`.
- **Indicators**: `bar`, `zone_bar`, `compass`, `compass_arrow`, `asi` (air speed), `msi`, `msi2` (motor speed), `cairo_gauge_marker`, `cairo_gauge_round_annotated`, `cairo_gauge_arc_annotated`, `cairo_gauge_donut`.

In our format the names may be rationalized (e.g. dropping the `cairo_` prefix); the importer maps the original names.

### 4.4 Metrics and units

Metrics (from the original): `speed`, `cspeed`, `accel`, `gradient`, `cgrad`, `alt`, `odo`, `codo`, `dist`, `azi`, `cog`, `lat`, `lon`, `timestamp`, `gps-dop`, `gps-lock`, `gps-packet`, `gps-packet-index`, `accl.x/y/z`, `grav.x/y/z`, `ori.pitch/roll/yaw`, `hr`, `cadence`, `power`, `temp`, `respiration`, `gear.front`, `gear.rear`, `sdps`.
Derived metrics (`cspeed`, `cgrad`, `codo`, `accel`, `azi`, `cog`, …) are computed as in the original, filtering out points with high DOP. Metrics not available from a source are absent (the widget shows "--" or the last value in grey, as chosen).

Units tied to the physical quantity:
- speed: km/h, mph, knots, m/s, pace (min/km, min/mi, min/nm), spm;
- distance: km, mi, nmi, m;
- altitude: m, ft;
- temperature: °C, °F;
- acceleration: G, m/s².

Each widget inherits the layout's unit system and can override it.

### 4.5 Render engine
- **tiny-skia** (CPU, antialiasing, paths): also covers the Cairo-style indicators.
- Text with **cosmic-text / rustybuzz** (full shaping, replaces libraqm). Embedded fonts (Roboto, Apache-2.0).
- Static parts (backgrounds, scales, icons, map route) are rendered once and cached; for each t only values, needles, and markers are redrawn.
- **Icons**: the original's icons come from Flaticon and have no free license → **they are not carried over**. A freely licensed set is used (e.g. Tabler Icons, MIT) with semantic equivalents (mountain, gradient, thermometer, heart, tachometer, GPS…); the importer maps the original files to semantic names.

### 4.6 Importer for the original layouts
- Original XML → `*.ovl.json`. Reference resolution taken from the file name (`default-1920x1080.xml`) or asked of the user.
- Automatic anchoring of each top-level group to the nearest corner/edge; conversion of Python format strings and unit names; icon mapping.
- Report of non-convertible elements (never silently dropped).
- The 13 layouts included in the original are converted once and shipped with the app (already anchored), with a copyright/provenance notice.

## 5. Editor

- Selection from the video or from the layer tree; selection boxes computed by `render` at the current instant.
- Moving: edits `offset` (moving a group moves its children). Option: automatic anchor update based on the quadrant where the node is dropped.
- Resizing on the widget's own parameter: `size` with locked proportions (text, icons, maps, indicators) or width/height (frames, charts, bars).
- 9-point anchor selector; guide line toward the anchoring edge while dragging.
- Guides and snapping, multiple selection, copy/paste, undo/redo, widget palette.
- Preview at multiple resolutions (16:9, 4:3, 9:16, 4K) without changing video.
- Properties panel generated from the schema (§4.3).

## 6. Project, preferences, system integration

### 6.1 Project file (optional, `*.ovp.json`)
Video and chapters, layout reference, GPX/FIT, sync offsets, scale mode, privacy zones, export settings. Only meant for moving/sharing a piece of work.

### 6.2 Preferences and state
- **Global preferences** in the system configuration folder: last layout used (applied to new videos and preselected in export), last export settings, recent files, window/panels, unit system, map provider and API keys, global privacy zones.
- **Per-video state** in an internal app store (key: file identity): linked GPX/FIT, sync offset, chosen layout, playback position. No files are created next to the videos.
- Opening by dragging into the window or from "Open with".

### 6.3 File association
- **Windows**: at startup, if the app is not registered, it asks (with a **"Don't ask again"** option) to add itself to the **"Open with"** menu for the common action camera formats: `.mp4`, `.mov`, `.lrv`, `.insv`. Registration for the current user only (`HKCU\Software\Classes`, without administrator privileges) via `OpenWithProgids`; no attempt is made to become the default app (Windows prevents this programmatically). If the executable has been moved, the registered path is updated silently at startup. In preferences: an entry to remove the registration.
- **macOS**: document types declared in the bundle's `Info.plist` (no prompt to the user).
- **Linux**: optional "Integrate with system" action that installs a `.desktop` file with the MIME types in `~/.local/share/applications`.

## 7. Export and maps

**Export**
- Pipeline with parallel stages: HW decoding → multi-threaded overlay render (independent frames) → compositing → encoding.
- **Final video**: H.264/H.265 with HW encoder (VideoToolbox, NVENC/QSV/AMF, VA-API), x264/x265 fallback. Audio copied. Frame rate and resolution of the original.
- **Overlay only**: ProRes 4444 with alpha, or PNG sequence.
- Exportable range (in/out), progress with estimated time, cancellation that leaves a valid file.
- CLI: `actionlay export --layout L --out O VIDEO…` with the same crates.

**Maps**
- Configurable providers (OSM and the others from the original), API keys in preferences.
- Shared disk cache; background prefetch of the tiles of the route area when the video is opened.
- Compliance with the OSM usage policy (identifying user-agent, rate limit, no bulk downloads) and visible attribution.
- Offline: cached tiles are used, grey tiles where missing, no blocking errors.

## 8. Errors and tests

**Errors**: rotating log file (`tracing`) in the app folder, pointed out to the user in case of a crash. Non-blocking messages for non-fatal problems (missing HW, tiles, GPX out of range). Telemetry gaps handled as in §4.4.

**Tests**
- `telemetry`: real GPMF tracks (locally `samples/hero7-GX013370.gpmd.bin`, 840 KB, not committed; in CI, publishable tracks to be found) compared with the values produced by the original; GPX/FIT merging; GPSU; timelapse.
- `layout`: JSON round-trip, schema validation, import of **all 13 layouts** without errors.
- `render`: **reference images** generated with the original (Python, development tool only, not distributed) for each widget and layout, compared with a measured tolerance; own snapshots for regressions.
- `media`: 2–3 s clips cut from the real samples for precise seek, chapter transitions, A/V alignment.
- End-to-end CLI: 3 s export, track verification with ffprobe, comparison of sample frames.
- GitHub Actions CI on macOS, Windows x64, Linux x64.

**Large samples**: outside the repo (`samples/`, ignored by git), downloaded by a script. The first sample (`GX013370.MP4`, HERO7) is documented in `samples/README.md`. It contains real GPS positions: **it must not be published** (owner's decision). Neither the video nor the extracted telemetry may be committed; CI uses only synthetic or publishable samples.

## 9. Distribution and platforms

- **ffmpeg**: pinned stable release (at start: **n9.0.2**, aligned with the `ffmpeg-next`/`ffmpeg-sys-next` 9.0 bindings); statically built from a script in the repo with `--enable-gpl`, **never `--enable-nonfree`**, only the necessary codecs/formats. Building it statically in CI on three platforms is the **second infrastructure risk** after the player.
- Fonts, icons, and default layouts embedded in the binary.
- **Windows** 10 1809+ x64: a single `.exe` with static C runtime.
- **macOS** 12+: universal `.app` bundle (arm64 + x86_64) in a `.dmg`.
- **Linux** x64: an AppImage, glibc 2.31 baseline (Ubuntu 20.04); Vulkan or OpenGL from the system.
- Expected size: 40–70 MB per platform.
- Rust edition 2024, stable toolchain pinned with `rust-toolchain.toml`.
- Releases on GitHub Releases built by CI. **No code signing** for now (instructions in the README for Gatekeeper/SmartScreen); CI set up to add it.
- Licenses: everything compatible with GPL-3 (ffmpeg GPL-2.0-or-later, x264/x265 GPL-2.0-or-later, telemetry-parser Apache-2.0, MIT/Apache crates, Roboto Apache-2.0, Tabler MIT). Every release includes `THIRD_PARTY_LICENSES` (also visible in "About") and the source archive of ffmpeg and the linked libraries with the build options. H.264/H.265 patents: with HW encoders/decoders the issue lies with the manufacturer; software x264/x265 are distributed the way VLC/HandBrake/Shotcut do.
- Repository language: all repository documentation (README, design docs, code comments, and commit messages) is in **English** (the convention of open source Rust projects); the interface is localizable (English + Italian in v1).

## 10. Internal phases (development order)

Each phase has its own implementation plan. The public release happens only when all are complete.

1. **M0 – Player prototype (risk reduction)**: egui + wgpu + static ffmpeg, HW decoding, audio, on macOS and Windows, with the HERO7 sample at 100 fps and a 10-bit 4K H.265. Criterion: smooth playback and aligned A/V. If it fails: fall back to **static libmpv** (GPL, compatible) for playback, without changing the rest of the architecture. Includes the static ffmpeg build in CI.
2. **M1 – Telemetry**: `telemetry` crate + dump CLI; comparison with the original.
3. **M2 – Layout and basic render**: JSON format, schema, text/metric/icon/date/frame widgets, overlay in the player.
4. **M3 – Importer and all widgets**: maps included, reference images.
5. **M4 – Editor**.
6. **M5 – Export** (app + CLI).
7. **M6 – External sources and chapters**: GPX/FIT, other cameras, chapters, privacy zones.
8. **M7 – Polish and distribution**: preferences, "Open with", packages, licenses, v1.0.

## 11. Credits

- [gopro-dashboard-overlay](https://github.com/time4tea/gopro-dashboard-overlay) by time4tea — inspiration, know-how on GPMF, metrics, and widgets; original layouts converted.
- [telemetry-parser](https://github.com/AdrianEddy/telemetry-parser), [Gyroflow](https://github.com/gyroflow/gyroflow) (architectural reference), FFmpeg, OpenStreetMap contributors.
