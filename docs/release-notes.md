# ActionLay 1.4.0

ActionLay 1.4.0 adds Insta360 phone GPS files, automatic activity matching on
desktop and web, linked telemetry in exports and desktop video rotation controls.

## What’s new

### Linked activities on desktop and web

- **INSGPS support:** read Insta360 phone GPS files alongside GPX and FIT,
  retaining millisecond timestamps, coordinates, stored speed, course and altitude.
- **Multiple files and folders:** match activity sample times against the video's
  UTC reference. A single compatible candidate is linked automatically. Multiple
  candidates are listed with filenames and UTC ranges so you can choose. If none
  match, a clear warning suggests setting Video UTC or selecting a single file.
- **Single-file alignment:** use compatible video/activity timestamps, or align
  their beginnings when dates cannot be matched. A warning explains the fallback
  and suggests adjusting the activity offset if visual alignment needs correction.
  Positive offsets move activity data later in the video.
- **Export linked telemetry:** the selected activity and offset are used for both
  preview and export. Embedded camera values fill activity gaps. Progressive
  camera metadata updates preserve the linked activity in the browser.
- **Remember source settings:** desktop retains links and alignment per video.
  The browser retains alignment settings, but videos and activity files must be
  selected again after reloading.
- Source selection explains single-file alignment and automatic discovery from
  multiple files or a folder before you choose. The guidance, new controls and
  actionable warnings are translated in all 34 languages.

### Desktop video rotation

- **Automatic** respects the video's display-matrix orientation.
- Manual **0°**, **90° clockwise**, **180°** and **90° counterclockwise** overrides
  are remembered per video and apply to preview, editor canvas and export.
- CLI exports support `--rotation auto|0|90|180|270`.

See the [activity source guide](https://github.com/porech/actionlay/blob/v1.4.0/docs/activity-sources.md)
for matching rules, offset examples, the INSGPS representation and regression coverage.
The [browser guide](https://github.com/porech/actionlay/blob/v1.4.0/docs/web.md)
describes browser capabilities and limits.

## Download and install

- **Windows 10/11 x64:** use [`actionlay-1.4.0-windows-x64-setup.exe`](https://github.com/porech/actionlay/releases/download/v1.4.0/actionlay-1.4.0-windows-x64-setup.exe).
- **macOS 12 or newer, Apple Silicon and Intel:** use [`actionlay-1.4.0-macos-universal.dmg`](https://github.com/porech/actionlay/releases/download/v1.4.0/actionlay-1.4.0-macos-universal.dmg), then drag ActionLay to Applications.
- **Linux x64:** follow the [signed APT/DNF repository instructions](https://porech.github.io/actionlay/packages/), using the stable channel.
- **Browser:** [open ActionLay Web](https://porech.github.io/actionlay/web/). Processing stays on your device.

Portable Windows/Linux builds, native DEB/RPM packages, dependency sources,
the browser bundle and `SHA256SUMS` are also attached. Builds are not Developer ID
signed or Apple-notarized; macOS and Windows may require allowing an unknown publisher.

## Known limits

- The INSGPS parser supports the packed 53-byte representation observed in an
  Insta360 phone sample. Other models, app versions or firmware variants are not
  guaranteed. Speed values are preserved; no official-software stationary filter
  is reproduced.
- Timestamp matching establishes temporal compatibility, not proof of camera
  identity. Incorrect recording clocks can prevent automatic matching. Long
  telemetry gaps do not qualify solely because the outer date range overlaps.
- The browser depends on browser codecs and WebCodecs. Export supports H.264/H.265
  MP4 and source AAC audio when supported. ProRes, transparent overlays, PNG
  sequences and manual video rotation controls remain desktop features. Without
  direct file saving, downloads are buffered in memory and limited to 256 MB.
  Map providers must allow browser CORS requests.
- Desktop export processes one source file with GoPro metadata and linked
  GPX/FIT/INSGPS activities, using eight-bit SDR decoding. Other embedded native
  camera telemetry and joined chapter timelines are not included in export;
  HDR/ten-bit preservation is not implemented. Other embedded camera telemetry
  and joined timelines are not connected in the browser.
- Insta360 playback shows raw video, without 360 stitching or reframing.
- Privacy zones affect maps; they do not redact footage or other GPS widgets.

[All changes since 1.3.0](https://github.com/porech/actionlay/compare/v1.3.0...v1.4.0)
