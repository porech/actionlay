ActionLay 1.0.1 makes installation and updates easier on Windows, macOS and Linux.

- Windows installer built with Inno Setup: per-user or all-users installation, optional desktop shortcut, Start Menu entry, upgrades and uninstall from Windows Apps.
- Signed APT and DNF repositories, with native DEB/RPM packages and application-menu entries.
- Universal macOS DMG with a branded drag-to-Applications window.
- Open With support for MP4, MOV, LRV and INSV on all desktop platforms. Installer/package registrations are detected automatically; default players are preserved.
- macOS opens files passed by Finder both at startup and while ActionLay is running.

Download the package for your system:

- **macOS 12+**, Apple Silicon and Intel: open `actionlay-macos-universal.dmg`, then drag ActionLay to Applications.
- **Windows 10/11 x64**: run `actionlay-windows-x64-setup.exe`.
- **Linux x64**: follow the [APT/DNF repository instructions](https://porech.github.io/actionlay/). Ubuntu 22.04+, Mint 21+, Debian 12+ and Fedora-compatible systems with glibc 2.35+ are supported. A working graphics driver is required.

Standalone Windows ZIP and Linux tar.gz builds, individual DEB/RPM packages, source archives and `SHA256SUMS` are also attached to this release.

The telemetry CLI is included. On macOS it is inside `ActionLay.app/Contents/MacOS/`.

Builds are not signed by an identified publisher. macOS may require allowing ActionLay in System Settings → Privacy & Security; Windows may show an unknown-publisher prompt.

Known limits: export currently processes one file with embedded GoPro telemetry, uses eight-bit SDR decoding, and does not yet include linked activities, native-camera telemetry or joined chapter timelines. Privacy zones hide map positions/routes; they do not redact video or other GPS widgets. Insta360 playback shows the raw video, without 360 stitching/reframing. Camera/firmware telemetry support varies.
