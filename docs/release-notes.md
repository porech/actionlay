ActionLay 1.3.0 introduces a browser version, lets you copy whole dashboard sections between layouts, and improves historical telemetry loading across desktop and web.

## What’s new

### Browser version

- Play local videos with telemetry overlays, edit layouts and export H.264/H.265 MP4 using browser media APIs. Processing runs on your device; video files are not uploaded to a processing server.
- Import and download layouts, including packaged fonts and images. Preferences and up to ten recent layouts persist in localStorage; videos must be selected again after reloading.
- The interface follows the browser language and offers the same 34 language catalogues as desktop. Full-screen includes overlays; controls and the pointer hide after three seconds without mouse movement. Videos can be opened by dropping them onto the page.
- The web bundle is attached to stable releases and deployed to GitHub Pages. Development commits test the browser app without replacing the published stable version.
- See [browser capabilities and limitations](https://github.com/porech/actionlay/blob/v1.3.0/docs/web.md).

### Layout editor, on desktop and web

- **Copy sections between layouts:** select a widget or container, use Copy or Ctrl/Command+C, open another layout and paste. Children, geometry and packaged assets are preserved. Clipboard contents survive closing and reopening the editor within the same application session.
- **Lock selection:** the Layers tree can protect selected sections or multiple widgets from accidental selection of children or deselection by preview clicks. Move, resize and copy still work; selecting a level in the tree releases the lock.
- **Optional scaling of children:** drag the corner handle to resize only the container, or hold **Shift** to scale the section and its nested children proportionally. The instruction appears in the properties panel and the handle tooltip. Shift can change the mode during a gesture, and one undo restores the complete operation.
- **Moto:** GPS coordinates have been removed, leaving the map and compass higher up. **Training:** GPS coordinates have been replaced by a route map alongside the altitude chart.

### Historical telemetry and export

- Historical data requirements are shared across maps, charts, G-meters, filtered compasses and cumulative metrics. Each affected widget shows a percentage spinner until its own required data is ready.
- Playback reads current telemetry progressively. A separate indexed reader recovers full routes or journey charts independently of playback seeks; finite history windows follow the requested time. Previously loaded metadata is reused.
- Known empty metadata intervals count as read without inventing metric values. Linking activity data does not falsely mark camera telemetry as loaded.
- Export waits for complete, validated metadata when any visible widget needs the full recording, including journey charts without a map. Otherwise, it reads the necessary history as export advances rather than preloading unrelated future metadata. Loading indicators remain outside exported pixels.

### Development and reliability

- The [building and contributing guide](https://github.com/porech/actionlay/blob/v1.3.0/docs/building.md) covers desktop and web setup, shared architecture, tests, packaging and release workflows.
- Browser dependencies are isolated from the four desktop targets. Desktop binaries keep their native FFmpeg backend.
- Added regressions for cross-layout clipboard operations and assets, section selection and scaling, backward seeks and history coverage, individual widget loading indicators, and full-history versus progressive exports.

## Download and install

- **Windows 10/11 x64:** use [`actionlay-1.3.0-windows-x64-setup.exe`](https://github.com/porech/actionlay/releases/download/v1.3.0/actionlay-1.3.0-windows-x64-setup.exe).
- **macOS 12 or newer, Apple Silicon and Intel:** use [`actionlay-1.3.0-macos-universal.dmg`](https://github.com/porech/actionlay/releases/download/v1.3.0/actionlay-1.3.0-macos-universal.dmg), then drag ActionLay to Applications.
- **Linux x64:** follow the [signed APT/DNF repository instructions](https://porech.github.io/actionlay/), using the stable channel. Ubuntu 22.04+, Mint 21+, Debian 12+ and Fedora-compatible systems with glibc 2.35+ are supported. A graphics backend compatible with wgpu is required.

Portable Windows ZIP and Linux tar.gz builds, native DEB/RPM packages, dependency sources, the browser bundle and `SHA256SUMS` are also attached. The telemetry CLI is included; on macOS it is inside `ActionLay.app/Contents/MacOS/`.

Builds are not signed by an identified publisher or Apple-notarized. macOS may require allowing ActionLay in **System Settings → Privacy & Security**; Windows may show an unknown-publisher prompt.

## Known limits

- **Browser:** playback and export depend on the codecs supported by the browser. Telemetry currently comes from embedded GoPro GPMF; linked GPX/FIT activities, other camera telemetry and joined chapter timelines are not connected. The editor previews demonstration telemetry. Export supports H.264/H.265 MP4 and source AAC audio when available; ProRes, transparent output and PNG sequences remain desktop features. Without direct file saving, exports are buffered in memory and limited to 256 MB. Map providers must support browser CORS. Browser storage quotas can limit retained layouts.
- **Desktop export:** processes one file with embedded GoPro telemetry using eight-bit SDR decoding. Linked GPX/FIT activities, native-camera telemetry and joined chapter timelines are not yet included in export; HDR/ten-bit preservation is not implemented.
- Privacy zones hide map positions and route segments; they do not redact the video or other GPS widgets.
- Insta360 playback shows raw video, without 360 stitching or reframing. Telemetry availability varies by camera and firmware.

[All changes since 1.2.0](https://github.com/porech/actionlay/compare/v1.2.0...v1.3.0)
