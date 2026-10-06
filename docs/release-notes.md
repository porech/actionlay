ActionLay 1.0.0 adds telemetry dashboards to action-camera footage and lets you design and export your own overlays.

- GoPro telemetry, automatic chapter sequences, and linked GPX/FIT activities.
- Native DJI/Insta360 telemetry where supported by telemetry-parser.
- Visual layout editor, portable layouts with assets/fonts, and imported upstream XML dashboards.
- Configurable maps, route colours/orientation and global privacy zones.
- Video export, transparent ProRes/PNG overlays, and solid-colour backgrounds.
- A gecko icon illustrated from the project owner's photographs.

Download the package for your system:

- **macOS 13+**, Apple Silicon and Intel: open `actionlay-macos-universal.dmg`, then drag ActionLay to Applications.
- **Windows 10/11 x64**: extract `actionlay-windows-x64.zip` and run `actionlay.exe`.
- **Linux x64** (Ubuntu 22.04 or newer): extract `actionlay-linux-x64.tar.gz` and run `./actionlay`. ALSA, VA-API and a working graphics driver are required.

The telemetry CLI is included. On macOS it is inside `ActionLay.app/Contents/MacOS/`.

Builds are not signed by an identified publisher. macOS may require allowing ActionLay in System Settings → Privacy & Security; Windows may show an unknown-publisher prompt.

Known limits: export currently processes one file with embedded GoPro telemetry, uses eight-bit SDR decoding, and does not yet include linked activities, native-camera telemetry or joined chapter timelines. Privacy zones hide map positions/routes; they do not redact video or other GPS widgets. Insta360 playback shows the raw video, without 360 stitching/reframing. Camera/firmware telemetry support varies.
