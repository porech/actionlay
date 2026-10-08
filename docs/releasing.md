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
Each release also includes `third-party-sources-VERSION.tar.gz` containing the pristine
pinned FFmpeg/x264/x265 trees and the matching build/compatibility scripts.
The build job's artifacts are intermediate inputs; downloads are published as
GitHub Release assets once **all** builds and packaging checks succeed.

## Stable release

1. Update `[workspace.package].version` in `Cargo.toml` and the local workspace
   packages in `Cargo.lock` (Cargo updates them automatically).
2. Update `docs/release-notes.md`, the installer fallback version and the versioned
   README download filenames, commit and push. Wait for CI to pass.
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

The About dialog shows `VERSION-dev+SHORT_SHA` for development builds and
`VERSION` for matching stable tags. Local Git builds follow the same rule.

Download filenames include the workspace version, including DMG, installer and
portable archives. Installed executable names remain stable so shortcuts, file
associations and upgrades continue to work. Publishing nightly removes obsolete
assets after uploading the current set; stable release assets are immutable.

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

The **Publish Pages and Linux repositories** job is part of `ci.yml`. After
release publication succeeds, it reconstructs both
stable and nightly APT/DNF repositories from release assets. It verifies and signs
metadata with the dedicated `GPG_PRIVATE_KEY` secret, signs RPM packages, then
deploys GitHub Pages. Linux setup instructions live at
<https://porech.github.io/actionlay/packages/>; existing `/stable/` and `/nightly/`
repository URLs stay unchanged; place any new repository paths under `/packages/`.
The root hosts the project landing page, with platform recommendations based on
the latest stable release, independently of the tag-built browser bundle. Repository deployment
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

Pages always reuses the latest stable release's browser archive; main builds never
publish an untagged browser build. A stable tag changes the browser version;
successful main builds refresh Linux repositories while keeping the browser and
public installation instructions from the latest stable tag. Landing assets and
the instructions renderer are restored from that tag before rendering, so edits
in main become public only with the next stable release. After deployment, CI checks the served HTML,
entry assets and WASM against that archive, since a successful deployment alone
does not prove the new files are being served. The same CI workflow can also be
dispatched manually in **pages** mode to republish existing release assets without
rebuilding or replacing binaries. GitHub Pages can reuse an earlier deployment
when main and a tag share a commit SHA; if verification detects stale files, run
the workflow from a newer main commit and verify the public URLs again.

For that recovery, make the newer commit on `main`, then dispatch
`gh workflow run ci.yml --ref main -f mode=pages`. This downloads the already published
stable assets; it does not rebuild or replace the release packages. Wait for
the workflow's **Verify published browser bundle** step to succeed before
announcing the browser update. A documentation-only recovery commit can use
`[skip ci]` to avoid an unnecessary native rebuild while the manually dispatched
CI Pages job performs the publication checks.

Pages artifacts use a unique name per workflow run and attempt. Before deploying,
the job waits until GitHub's artifact listing contains the uploaded artifact ID.
This avoids duplicate-name failures on retries and upload/list visibility races;
a retry cannot choose an older attempt's Pages archive. The served browser bundle
is still checked against the selected stable release after deployment.


## Arch/AUR preparation

The **Check Arch/AUR package** job creates `actionlay-bin` from the tested Linux
portable binaries, with SHA-256-pinned sources. It builds with `makepkg` as a
non-root user in an Arch container, runs `namcap`, installs/reinstalls/removes with
pacman, checks executable dependencies, desktop/MIME registration and notices,
and uploads a preview recipe including `.SRCINFO`. This check gates release
packaging. The preview archive is generated before publication; its recipe must
not be submitted to AUR because final release archive checksums can differ.

AUR submission starts with a stable release after 1.4.2 and is initially disabled.
Once configured, the **Publish stable AUR recipe** job downloads the immutable
published archive, verifies `SHA256SUMS`, regenerates the recipe and `.SRCINFO`,
repeats the Arch checks, then commits only recipe files to the AUR Git repository.
See [AUR maintainer setup](../packaging/aur/README.md). The user-facing Arch
installation instructions should be added only after the first AUR submission is
live. Packages installed via pacman use system-managed updates; portable copies
remain eligible for in-app updates.

## Signed Arch repository

Starting with the next stable tag after 1.4.2, releases include the verified
`actionlay-bin-*.pkg.tar.zst` package. The existing Pages job signs both packages
and the pacman database with the same dedicated repository key used for APT/DNF.
The stable repository is hosted under `/packages/stable/arch/x86_64/`.
No AUR account or additional signing secret is needed. CI tests the recipe,
installation/removal and a signed repository with a real pacman client using a
temporary test key. The private production key never enters the Arch container.

The public instructions include Arch only when the stable package, signed
database and public key are available. Users enable the repository with
`PackageRequired DatabaseRequired`; updates then come through pacman. The app
recognizes pacman ownership and leaves updates to the package manager.

AUR publication remains an optional separate distribution route, described in
[packaging/aur/README.md](../packaging/aur/README.md). Registration is currently
closed; enabling AUR publication is not required for the signed pacman repository.
