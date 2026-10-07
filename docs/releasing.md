# Releases

CI tests and builds Windows x64, Linux x64, macOS ARM64 and macOS Intel.
Both Mac builds use a macOS 12 deployment target. Native builds are merged with
`lipo` into universal `actionlay` and `actionlay-telemetry` executables in
`ActionLay.app`, then packaged in a compressed DMG with an Applications shortcut.

The application has an ICNS icon; the DMG has a gecko-on-disk ICNS as its mounted
volume icon. This icon is stored inside the mounted filesystem, so it survives
release uploads and downloads. The unmounted .dmg file uses the system file icon.
The app is ad-hoc signed; it is not Developer ID signed or notarized.

[Apple's universal binary documentation](https://developer.apple.com/documentation/apple-silicon/building-a-universal-macos-binary)
describes the architecture merge. Native runners use the documented
[GitHub macOS runner architectures](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

All packages contain licence notices and a link to their exact build source.
Each release also includes `third-party-sources.tar.gz` containing the pristine
pinned FFmpeg/x264/x265 trees and the matching build/compatibility scripts.
The build job's artifacts are intermediate inputs; downloads are published as
GitHub Release assets once **all** builds and packaging checks succeed.

## Stable release

1. Update `[workspace.package].version` in `Cargo.toml` and the local workspace
   packages in `Cargo.lock` (Cargo updates them automatically).
2. Update `docs/release-notes.md`, commit and push. Wait for CI to pass.
3. Tag the tested commit, for example `git tag v1.0.0`, then `git push origin v1.0.0`.

The workflow verifies the tag matches the workspace version. It uploads all
packages and `SHA256SUMS` into a draft, then publishes the completed release
as the latest stable version. Existing stable releases are not overwritten.

## Development release

Every successful build of `main` updates the `nightly` prerelease with the
same package names. Its notes and source notices identify the exact commit.
The stable README download links always use `/releases/latest`; development
builds are linked separately and never become the latest stable release.
Pull requests and other branches do not publish releases.

## Local packaging and icons

```sh
python3 scripts/build-icons.py
bash scripts/package-macos.sh /path/to/arm/binaries /path/to/intel/binaries dist
python3 scripts/package-release.py --target windows --binaries /path/to/binaries
python3 scripts/package-release.py --target linux --binaries /path/to/binaries
```

The icon sources and generation instructions are in [assets/icons](../assets/icons/README.md).
Windows embeds the ICO and version metadata using
[winresource](https://docs.rs/winresource/0.1.31/winresource/).

## Native installers and Linux repositories

Windows installers use Inno Setup with one stable AppId across versions. The
wizard supports current-user or all-user (UAC) installation, an optional desktop
shortcut, and upgrading the existing installation. CI tests both scopes, a newer
installer version and uninstall without changing default file associations.
Installer-owned registrations use `ActionLay.InstalledVideo`; portable per-user
registrations use `ActionLay.Video`. The app detects installer registrations in
both HKCU and HKLM and does not replace them.

Linux builds produce DEB and RPM packages alongside the portable archive.
Packages install binaries, a launcher, icon, MIME declarations and notices under
`/usr`. Their launcher carries `X-ActionLay-Managed=true`, so the app does not
create duplicate per-user integration. Package install/remove refreshes MIME and
desktop databases. The glibc 2.35 baseline supports Ubuntu 22.04+, Mint 21+,
Debian 12+ and recent Fedora-compatible systems; it does not support musl Alpine.

The `Linux package repositories` job in `ci.yml` runs after the release publication
job succeeds and reconstructs both
stable and nightly APT/DNF repositories from release assets. It verifies and signs
metadata with the dedicated `GPG_PRIVATE_KEY` secret, signs RPM packages, then
deploys GitHub Pages at <https://porech.github.io/actionlay/>. Repository deployment
failures therefore fail the same CI run. The signing key and Pages permissions
are scoped to this job; pull requests and non-release branches skip it. A release without
native packages is not advertised as an available channel. The public key is
committed at `packaging/linux/repository-key.asc`; private keys must stay outside
the checkout. The current keyring and exports are in `~/.actionlay-release-signing`
for a separate backup. Never commit that directory or upload it as an artifact.

The macOS DMG background is in `assets/dmg`. Packaging writes Finder positions
and the background alias directly using pinned `ds-store`/`mac-alias` tools in a
temporary venv, without requiring Finder or UI scripting on CI. The installed
application does not depend on Python.
