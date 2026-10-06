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
