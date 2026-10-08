# ActionLay 1.4.1

ActionLay 1.4.1 makes telemetry source controls easier to find on desktop and web.

## What’s changed

- Rename **Video sources** to **Metric sources** throughout the interface, including
  menus and the source dialog. All 34 translations use the same terminology,
  including English.
- Add **Metric sources** to the desktop and browser toolbar.
- Opening metric sources without a video now shows an instruction to open a video
  first, instead of silently doing nothing. This works from the menu, toolbar and
  browser source section.
- When the video already provides extracted metrics, source controls explain that
  an external source file can override them. The notice follows progressive
  telemetry loading and is translated in all 34 languages.

The INSGPS support, automatic activity matching, alignment and export behavior
introduced in 1.4.0 are retained.

See the [activity source guide](https://github.com/porech/actionlay/blob/v1.4.1/docs/activity-sources.md)
for matching rules, offset examples, the INSGPS representation and regression coverage.
The [browser guide](https://github.com/porech/actionlay/blob/v1.4.1/docs/web.md)
describes browser capabilities and limits.

## Download and install

- **Windows 10/11 x64:** use [`actionlay-1.4.1-windows-x64-setup.exe`](https://github.com/porech/actionlay/releases/download/v1.4.1/actionlay-1.4.1-windows-x64-setup.exe).
- **macOS 12 or newer, Apple Silicon and Intel:** use [`actionlay-1.4.1-macos-universal.dmg`](https://github.com/porech/actionlay/releases/download/v1.4.1/actionlay-1.4.1-macos-universal.dmg), then drag ActionLay to Applications.
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

[All changes since 1.4.0](https://github.com/porech/actionlay/compare/v1.4.0...v1.4.1)
