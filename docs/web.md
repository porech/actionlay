# Browser application

ActionLay Web is a static application in `web/`, backed by the target-isolated
`actionlay-web` Rust crate. CI builds and tests it on branches and pull requests. Stable `v*` tags package
`actionlay-web-VERSION.tar.gz` as a release asset. The Pages job installs that
released bundle at `/actionlay/web/`, alongside the signed Linux repositories.
Nightly/main deployments reuse the latest stable browser bundle; they never
publish an untagged web build. Open the hosted application at
<https://porech.github.io/actionlay/web/>.

## Build and run

Use the repository Rust toolchain and Node 24. Install the matching bindings CLI:

```sh
cargo install wasm-bindgen-cli --version 0.2.129 --locked
bash scripts/build-web.sh
npm --prefix web run dev
```

The generated bundle is `web/dist/`. It can be hosted under any directory; URLs
are relative. Serve over HTTPS (or localhost) for browser media and filesystem
APIs. There is no processing server, upload endpoint, native bridge, or requirement
for cross-origin isolation/SharedArrayBuffer. FFmpeg is not needed for this build.

## Architecture

- Layout models, package validation, GPMF decoding, metrics and the tiny-skia
  renderer are shared with desktop. Portable package I/O accepts seekable streams;
  native saves retain their atomic filesystem transaction.
- The visual editor and translations reuse the desktop sources. Browser file
  selection and downloads live in the browser shell; editing uses demonstration
  telemetry rather than the currently playing video.
- A dedicated Web Worker reads the MP4 index, then only the GPMF sample ranges.
  Telemetry is published progressively during playback; seeking prioritises indexed
  samples at the requested point. An independent history reader recovers the
  intervals declared by `actionlay-layout::history`: full source for journey charts
  and fitted/full-route maps, windows for charts, and prefixes for cumulative
  metrics, filtered compasses and G-meter calibration/peaks. Full reads continue
  independently of playback seeks; finite windows follow the current requirements.
  Both readers share a packet cache. Every historical widget shows its own
  percentage spinner until its required data is ready. Read coverage is distinct
  from missing metric values; loading indicators are never part of export pixels.
  It renders previews and runs export. Mediabunny supplies browser demuxing,
  WebCodecs decoding/encoding and MP4 muxing. Export awaits backpressure instead
  of keeping a decoded movie in memory. Export preloads and validates full metadata
  when any visible widget requires the full source; otherwise it acquires the
  necessary history before each frame while advancing sequentially.
- Playback uses a native HTML video element with shared fullscreen controls for
  the video, overlays and map loaders. `requestVideoFrameCallback` provides
  presentation time where available; preview overlays are rendered at up to 25 Hz.
  Preview resolution is capped independently of export resolution.
- Native maps keep their existing thread and disk-cache implementation. Browser
  maps use asynchronous fetch and a bounded memory cache. Providers must allow
  browser CORS requests; turning downloads off also disables network map requests.
- Preferences and up to ten recent complete `.actionlay-layout` packages (including
  assets) are stored under `actionlay.web.v1` in localStorage. The storage budget
  is three million JSON characters; browser quotas may be lower. Quota failures
  are reported and preserve the last successful stored state. Large layouts can
  still be downloaded. Videos are not stored there and must be selected again
  after reloading.
- Desktop is still the default workspace build. Web dependencies are target-gated,
  and desktop never depends on `actionlay-web`. The CI isolation check rejects
  native media libraries in the WASM tree and browser bindings
  in all four desktop dependency trees (macOS ARM/Intel, Windows and Linux).
  The check runs in the web job and each desktop CI job.

## Current format coverage

Import/export `.actionlay-layout` packages, import standalone `.ovl.json` layouts
without external assets, and import upstream XML layouts. Asset-bearing JSON
should be packaged on desktop first. The browser editor supports asset uploads,
selection, properties, dragging, resizing, grouping, and undo/redo; device file
access uses upload/download rather than arbitrary paths.

Video playback follows the browser's container/codec support. Embedded GoPro GPMF
is extracted, including fragmented MP4 indexes. Other camera telemetry, linked
GPX/FIT activities, automatic chapter discovery and joined timelines are not yet
connected in the browser shell.

Export supports H.264 or H.265 MP4 when the browser encoder supports the source
resolution, either video with overlay or a solid-colour overlay. It keeps source
AAC audio for video exports. Audio packets are copied; a trim can have an audio
boundary offset of up to one packet. Other audio codecs produce a clear error.
Quality presets, advanced encoding controls, ProRes, transparent output and PNG
sequences remain desktop features. The browser remembers the selected export mode
and codec in localStorage.

Where direct file saving is available, export streams into a filesystem transaction; success
commits it and cancellation aborts it, preserving an existing destination. Other
browsers receive a download buffered in memory, limited to 256 MB. Map loading
has a per-frame deadline; unresolved tile requests fail the export rather than
silently committing an incomplete overlay.

## Verification

```sh
bash scripts/check-backend-isolation.sh
npm --prefix web test
cd web
npx playwright install chromium
npm run test:browser
```

Browser integration tests use the production bundle under `/web/`, including
preferences and layout assets across reload, shared editor startup, draft discard,
playback/seek, trimmed export with AAC audio, streamed export and cancellation.
FFmpeg/ffprobe are used only to generate and inspect the test video, not by the
web application. Existing native layout, rendering, telemetry and application
checks remain part of desktop CI.

For desktop regression checks, provide the bundled `FFMPEG_DIR` and run
`make check` and `make test` with the public GoPro and synthetic samples present.
CI requires the public GoPro samples so missing fixtures cannot silently skip
those tests. Renderer golden images must pass against the existing references;
do not regenerate the references to accept changes from the browser port.
Package tests also compare the memory encoder with the native file encoder,
including assets, unknown fields and the exact ZIP bytes.

The CI desktop matrix builds and tests macOS ARM/Intel, Windows and Linux.
Local macOS results do not replace those other runners. Before release, perform
an interactive desktop check of video/audio playback, seeking and chapter
transitions, full-route maps, editor undo/redo and asset import, saving/reopening
layouts, and trimmed video/solid-background export. Use the same source video
and layout before and after the change for any visual or timing comparison.
