ActionLay 1.2.0 adds immersive full-screen playback, remembered window geometry, export quality presets and clearer telemetry loading progress. It also improves recovery from buffering and slow video output, and refreshes the Windows installer and macOS installation window.

## What’s new

### Playback and window behaviour

- **About ActionLay** now has a coordinated gecko design, the shared slogan, licence/project links and the application version (development builds include the short Git commit): in the native ActionLay menu on macOS, and in the ActionLay menu on Windows and Linux.

- **Full-screen playback** with a dedicated transport overlay. Controls and the mouse pointer hide after **3 seconds** without movement and reappear when you move the mouse. Use **F11** or the full-screen button to toggle it, and **Escape** to exit. Full-screen is available during playback, outside the layout editor.
- The app **remembers window position, size and maximized state**. A first launch opens a centered window smaller than the usable screen area; restored geometry is kept within available displays.
- **Software video decoding** can be selected under **Settings → Advanced**, alongside the playback diagnostics setting. This is a decoder preference; it does not replace the graphics backend used to display the interface.

### Export quality and preparation

- **Balanced, High quality and Fast export** presets, with remembered export settings. Balanced uses software encoding and preserves the supported source codec, MP4/MOV container, resolution and video bitrate when available. This still re-encodes the video to add the overlay; it is not lossless packet copying.
- An **Advanced** section controls codec, container, resolution, quality or bitrate, maximum bitrate, buffer size, keyframe interval and encoder preference/speed. Edited options are highlighted and can individually return to the selected preset’s defaults.
- Selecting a preset resets its encoding options while keeping the output mode and background. Previously edited settings reopen Advanced on the next export. CLI export supports the same presets.
- Export now explicitly shows **metadata loading and preparation before frame rendering**, with a separate progress bar. Complete-route requirements are satisfied before the first frame is exported.

### Maps and telemetry

- Map widgets show a **spinner while loading the telemetry needed for the entire route, fitted zoom or the completed route after a seek**. A percentage is shown when the metadata index provides a packet count. These indicators are interface elements and never appear in exported video.
- Metadata progress uses the existing index, without extra reads of video or audio payloads.
- Full-route backfill explicitly signals validated completion, allowing fitted zoom to activate without waiting for playback to reach the end of the video. Included maps fit and center the complete route to 80% of the viewport; older saved layouts retain their own zoom settings.

## Fixes and reliability

- Improved audio-clock stability across latency jitter and buffering, keeping the displayed playback position monotonic between seeks.
- Playback waits for decoded audio and video to recover, rather than relying only on compressed packets being buffered. This reduces starvation and growing A/V drift after slow reads.
- Overdue video frames are drained across bounded presentation queues. Decoding preserves reference frames while skipping unnecessary downloads/conversions of output that is already late; seek preroll and the final frame at EOF are retained.
- Output recovery now accounts for the measured recent cost of frame materialization. Slow conversion no longer repeatedly sends the player back into buffering merely because a frame was usable before conversion but late when it finished.
- Recovery regressions now count actual post-seek buffering transitions, rather than using decoded timestamps that could hide presentation stalls. macOS Intel testing exercised real audio at both 44.1 and 48 kHz; the release pipeline also passed on macOS ARM, Windows and Linux before tagging.
- Reviewed all 34 language catalogues in context: repaired joined or shifted labels, chapter counts, the action to keep only one file open, GPS states, slope and typography terminology, and incomplete error messages. Catalogue checks now protect placeholders, technical identifiers and single-line labels from translation regressions.
- Already buffered packets now replenish decoded audio before costly video-frame conversion, reducing audio starvation during recovery. The slow-output regression keeps its real audio clock and strict recovery assertions; both macOS architectures pass.

## Installation and distribution

- Download filenames now include **1.2.0**, including the Windows installer, universal macOS DMG and portable archives. Installed application names remain stable for upgrades and file associations.

- **Windows installer:** welcome/completion artwork and the header icon now use the ActionLay gecko and coordinated teal palette. Per-user/all-users installation, upgrades, uninstall, the Start Menu entry and optional desktop shortcut remain supported.
- **macOS DMG:** the installation window now displays the branded background, drag direction and shared slogan: **Your videos. Your telemetry. No strings attached.** The window leaves room for Finder’s bars so the instructions remain visible. The portable background alias and icon positions are verified after the final DMG is compressed and remounted.
- Signed Linux repositories are now deployed **within the main release pipeline**, after release publication. A repository deployment failure is reported in the same CI run. Pages actions have been updated to Node 24.
- A coordinated GitHub social preview and editable vector sources are included in the repository.

## Download and install

- **Windows 10/11 x64:** use [`actionlay-1.2.0-windows-x64-setup.exe`](https://github.com/porech/actionlay/releases/download/v1.2.0/actionlay-1.2.0-windows-x64-setup.exe).
- **macOS 12 or newer, Apple Silicon and Intel:** use [`actionlay-1.2.0-macos-universal.dmg`](https://github.com/porech/actionlay/releases/download/v1.2.0/actionlay-1.2.0-macos-universal.dmg), then drag ActionLay to Applications.
- **Linux x64:** follow the [signed APT/DNF repository instructions](https://porech.github.io/actionlay/), using the stable channel. Ubuntu 22.04+, Mint 21+, Debian 12+ and Fedora-compatible systems with glibc 2.35+ are supported. A graphics backend compatible with wgpu is required.

Portable Windows ZIP and Linux tar.gz builds, native DEB/RPM packages, dependency sources and `SHA256SUMS` are also attached. The telemetry CLI is included; on macOS it is inside `ActionLay.app/Contents/MacOS/`.

Builds are not signed by an identified publisher or Apple-notarized. macOS may require allowing ActionLay in **System Settings → Privacy & Security**; Windows may show an unknown-publisher prompt.

## Known limits

- Export processes **one file with embedded GoPro telemetry** using eight-bit SDR decoding. Linked GPX/FIT activities, native-camera telemetry and joined chapter timelines are not yet included in export; HDR/ten-bit preservation is not implemented.
- Privacy zones hide map positions and route segments; they do not redact the video or other GPS widgets.
- Insta360 playback shows raw video, without 360 stitching or reframing. Telemetry availability varies by camera and firmware.

[All changes since 1.1.0](https://github.com/porech/actionlay/compare/v1.1.0...v1.2.0)
