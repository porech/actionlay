# Credits

ActionLay stands on the shoulders of these open-source projects:

- **[gopro-dashboard-overlay](https://github.com/time4tea/gopro-dashboard-overlay)**
  by time4tea is the main inspiration for this project, and the source of its
  know-how on GoPro telemetry, metrics and dashboard widgets. Its layouts can
  be imported into ActionLay.
- **[Gyroflow](https://github.com/gyroflow/gyroflow)** is the architectural
  reference for a Rust desktop app that plays and processes action-camera
  footage.
- **[gpmf-parser](https://github.com/gopro/gpmf-parser)** by GoPro documents
  the GPMF telemetry format; its sample videos (Apache-2.0) are ActionLay's
  telemetry test files.
- **[telemetry-parser](https://github.com/AdrianEddy/telemetry-parser)** by
  AdrianEddy is the telemetry reader for DJI, Insta360 and other
  cameras.
- **[GeographicLib](https://geographiclib.sourceforge.io)** (Charles Karney),
  through [geographiclib-rs](https://github.com/georust/geographiclib-rs),
  computes distances and bearings exactly as gopro-dashboard-overlay does.
- **[Mediabunny](https://mediabunny.dev/)** (MPL-2.0) supplies browser media
  demuxing, WebCodecs integration and MP4 export. **[MP4Box.js](https://github.com/gpac/mp4box.js)**
  (BSD-3-Clause) indexes embedded browser telemetry tracks.
- **[FFmpeg](https://ffmpeg.org)** does the decoding, through the
  [ffmpeg-next](https://github.com/zmwangx/rust-ffmpeg) Rust bindings.
- **[tiny-skia](https://github.com/linebender/tiny-skia)** draws the overlay
  and **[cosmic-text](https://github.com/pop-os/cosmic-text)** shapes its text.
- **[Roboto](https://github.com/googlefonts/roboto)** (Apache-2.0) is the
  overlay font, and **[Tabler Icons](https://tabler.io/icons)** (MIT) provide
  its icons.
- **[Noto fonts](https://github.com/google/fonts)** (SIL OFL 1.1), embedded as
  compact subsets, provide multilingual interface glyphs.
- **[egui / eframe](https://github.com/emilk/egui)** and
  **[wgpu](https://github.com/gfx-rs/wgpu)** power the user interface and the
  GPU rendering.
- **[cpal](https://github.com/RustAudio/cpal)** and
  **[ringbuf](https://github.com/agerasev/ringbuf)** handle audio output.
- **[OpenStreetMap](https://www.openstreetmap.org/copyright)** contributors
  provide the map data for the map widgets.

