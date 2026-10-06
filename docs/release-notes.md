ActionLay 1.1.0 adds a multilingual interface, regional preferences, complete-route map framing and adjustable buffering. It also keeps telemetry visible through slow network reads and makes export preparation more reliable.

## What’s new

### Interface and regional preferences

- **34 interface languages**, with native language names and a searchable selector under **Settings → Interface**. **System default** is the first option and the initial selection: it follows changes to the system language, with English as the fallback for unsupported languages. You can also choose a language explicitly, without restarting.
- The **Show diagnostic data** switch now lives in Interface settings.
- **Settings → Regional settings** controls metric or imperial measurement preferences independently of the interface language. Choose **System default** to follow the system’s regional measurement settings, or select a fixed preference.
- Layouts and widgets now offer **Default** for units. A widget inherits its layout’s units; a layout set to Default inherits the regional preference. Explicit widget and layout units take priority. Included layouts use Default.

### Maps and telemetry

- Included layouts with a map now show the **entire route**: dark green for the completed section and yellow for the section ahead, with north up. The two additional-data loading warnings have been removed.
- Map widgets can use **fixed zoom** or **fit the entire route**, with configurable coverage. Included maps center the route and fit its bounds to **80% of the map viewport** along the limiting dimension.
- Route fitting works independently of the route line mode: it loads the complete route even when only the completed section is drawn. Preview keeps fixed zoom until loading finishes.
- Slow storage reads and buffering preserve the last valid displayed telemetry. Short gaps in telemetry also retain the previous value for a **configurable duration per widget**, set to **3 seconds** in included layouts; longer gaps show the widget’s fallback. Paused and buffering time does not consume that tolerance.

### Playback and export

- **Settings → Advanced** exposes read-ahead duration, the buffer required before playback resumes, and the memory limit for compressed packets. Defaults remain **3 seconds**, **2 seconds** and **32 MiB**. Changes apply to the open video and are saved; a button restores the defaults. Read-ahead starts when a video opens, even while paused. Packet and decoded-frame limits can also constrain the amount buffered; the memory setting is not a cap on the app’s total memory use.
- Export defaults to **Video with overlay**.
- When export needs a route line or complete-route zoom, it loads and validates the complete route **before rendering the first frame**. An incomplete metadata read stops export rather than producing a partial route. Cancellation during this preparation is handled as cancellation.
- Export progress shows elapsed and estimated remaining time in seconds, minutes and seconds, or hours, minutes and seconds, as appropriate. The remaining-time estimate excludes the initial metadata-loading phase.
- An export keeps the measurement units selected when it starts, even if regional preferences change while it is running.

## Download and install

- **Windows 10/11 x64:** use [`actionlay-windows-x64-setup.exe`](https://github.com/porech/actionlay/releases/download/v1.1.0/actionlay-windows-x64-setup.exe). The installer supports per-user or all-users installation, upgrades and uninstall through Windows Apps. It always creates a Start Menu entry; the desktop shortcut is optional.
- **macOS 12 or newer, Apple Silicon and Intel:** use [`actionlay-macos-universal.dmg`](https://github.com/porech/actionlay/releases/download/v1.1.0/actionlay-macos-universal.dmg), then drag ActionLay to Applications.
- **Linux x64:** follow the [signed APT/DNF repository instructions](https://porech.github.io/actionlay/), using the stable channel. Ubuntu 22.04+, Mint 21+, Debian 12+ and Fedora-compatible systems with glibc 2.35+ are supported. A graphics backend compatible with wgpu is required.

Portable Windows ZIP and Linux tar.gz builds, individual DEB/RPM packages, dependency sources and `SHA256SUMS` are also attached. The telemetry CLI is included; on macOS it is inside `ActionLay.app/Contents/MacOS/`.

Builds are not signed by an identified publisher or Apple-notarized. macOS may require allowing ActionLay in **System Settings → Privacy & Security**; Windows may show an unknown-publisher prompt.

## Known limits

- Export currently processes **one file with embedded GoPro telemetry** and uses eight-bit SDR decoding. Linked GPX/FIT activities, native-camera telemetry and joined chapter timelines are not yet included in export. The new complete-route preparation applies within this existing export scope.
- Privacy zones hide map positions and route segments; they do not redact the video or other GPS widgets.
- Insta360 playback shows raw video, without 360 stitching or reframing. Telemetry availability varies by camera and firmware.

[All changes since 1.0.1](https://github.com/porech/actionlay/compare/v1.0.1...v1.1.0)
