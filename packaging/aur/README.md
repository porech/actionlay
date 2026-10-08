# AUR maintainer preparation

This prepares **actionlay-bin** for x86_64, using ActionLay's CI-tested portable
Linux release archive. AUR hosts the build recipe; `makepkg` downloads the exact
versioned upstream archive and verifies its SHA-256 checksum. The package installs
the app and telemetry CLI, icon, desktop/MIME declarations and licence notices.
System pacman hooks refresh the desktop and MIME databases. Pacman-owned
executables are excluded from in-app updates, including AUR packages.

This support is being prepared after 1.4.2. No AUR submission or public Arch
installation instructions are published as part of 1.4.2.

## Before the first submission

1. Create/use the intended AUR maintainer account and register a dedicated SSH
   public key. Confirm that `actionlay-bin` is available or that the account has
   permission to update it.
2. Add the corresponding private key as the repository Actions secret
   `AUR_SSH_PRIVATE_KEY`. It is used only by the publish job, after package checks.
3. Set Actions variables `AUR_GIT_NAME` and `AUR_GIT_EMAIL` to the maintainer's
   commit identity. Set `AUR_SSH_KNOWN_HOSTS` to the verified AUR host-key entry;
   verify it through Arch's published host-key information before trusting it.
   The job requires strict SSH host-key verification.
4. Set `AUR_PUBLISH_ENABLED=true` only when that account/key setup is complete.
   The next successful stable release after 1.4.2 will submit automatically.
5. After the first submission succeeds, verify the public AUR page and install
   from its clone in a clean Arch environment, then update the site's installation
   instructions. Do not advertise AUR before the package exists.

The AUR job lives in the main `ci.yml` workflow. To retry a submission for an
already published stable release without rebuilding binaries:

```sh
gh workflow run ci.yml --ref main -f mode=aur -f release_tag=v1.4.3
```

The tag is an example: it must already exist, be stable and be later than 1.4.2.
The job checks out that tag so the recipe files match the release, downloads the
published Linux archive and `SHA256SUMS`, verifies them, builds/tests the package,
and submits `PKGBUILD`, `.SRCINFO` and the local MIME source to AUR. Binary packages
and downloaded archives are never committed to AUR. Repeating an unchanged
submission produces no new commit.

## Local recipe checks

```sh
python3 scripts/prepare-aur.py --version VERSION \
  --archive /path/to/actionlay-VERSION-linux-x64.tar.gz \
  --checksums /path/to/SHA256SUMS --output /tmp/actionlay-aur-recipe
bash scripts/verify-aur.sh /tmp/actionlay-aur-recipe \
  /path/to/actionlay-VERSION-linux-x64.tar.gz /tmp/actionlay-aur-verified
```

Docker is needed for local checks. The test container is ephemeral; packages and
pacman configuration are not installed on the host. Its download sandbox is
disabled to support nested containers and architecture emulation. Recipe checksums
remain enforced. `namcap` errors fail the check; warnings about unstripped upstream
executables and libraries loaded at runtime are expected.

The generated `.SRCINFO` comes from `makepkg --printsrcinfo`, rather than a second
hand-maintained metadata file. See the official
[PKGBUILD reference](https://man.archlinux.org/man/PKGBUILD.5.en) and
[makepkg reference](https://man.archlinux.org/man/makepkg.8.en).
