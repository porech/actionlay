# ActionLay 1.4.2

ActionLay 1.4.2 adds automatic updates to stable desktop builds.

## What’s changed

- Check for newer stable releases at startup without blocking the interface.
  Choose **Yes**, **Not now**, **Skip this version**, or **Don't ask again**.
- Restore skipped-version prompts or enable startup checks again under
  **Settings → Advanced → Automatic updates**. Choices survive application restarts.
- Show download progress and verify the release asset's SHA-256 checksum before
  installing. ActionLay closes for installation and restarts afterward.
- Windows installer updates preserve the installation directory and current-user
  or all-user mode, requesting UAC for all-user installations. Portable copies
  replace only their executable.
- macOS updates replace the running `.app` at its actual location, including
  custom directories and names. Standalone copies replace only their executable.
- Linux portable copies replace their executable. DEB/RPM installations use system
  updates and explain this in preferences instead of offering in-app updates.
- Development builds never check for updates; prereleases are never offered.
- Translate the update interface into all 34 supported languages and refresh the
  bundled font subsets.

Save edited layouts and finish exports before updating. Portable replacements
need a writable executable directory. Downloads that fail validation leave the
current installation intact; installation failures are reported on the next start.
The auto-updater starts with 1.4.2: users of older versions must install this
release manually once. The browser continues to update through the published web app.

## Download and install

- **Windows 10/11 x64:** use [`actionlay-1.4.2-windows-x64-setup.exe`](https://github.com/porech/actionlay/releases/download/v1.4.2/actionlay-1.4.2-windows-x64-setup.exe).
- **macOS 12 or newer, Apple Silicon and Intel:** use [`actionlay-1.4.2-macos-universal.dmg`](https://github.com/porech/actionlay/releases/download/v1.4.2/actionlay-1.4.2-macos-universal.dmg), then drag ActionLay to Applications.
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

[All changes since 1.4.1](https://github.com/porech/actionlay/compare/v1.4.1...v1.4.2)
