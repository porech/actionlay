#!/usr/bin/env bash
# Signed APT and DNF repositories; separate stable and development channels.
set -euo pipefail
PACKAGES="$(cd "${1:?package directory}" && pwd)"
SITE="${2:?site directory}"
CHANNEL="${3:?stable or nightly}"
[[ "$CHANNEL" == stable || "$CHANNEL" == nightly ]]
: "${GPG_FPR:?signing fingerprint required}"
mkdir -p "$SITE/$CHANNEL/apt" "$SITE/$CHANNEL/rpm"
cp "$PACKAGES"/*.deb "$SITE/$CHANNEL/apt/"
cp "$PACKAGES"/*.rpm "$SITE/$CHANNEL/rpm/"
APT="$(cd "$SITE/$CHANNEL/apt" && pwd)"
RPM="$(cd "$SITE/$CHANNEL/rpm" && pwd)"
(
  cd "$APT"
  dpkg-scanpackages --multiversion . > Packages
  gzip -9 -k -f Packages
  apt-ftparchive -o APT::FTPArchive::Release::Origin=ActionLay \
    -o APT::FTPArchive::Release::Label=ActionLay \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o APT::FTPArchive::Release::Description="ActionLay $CHANNEL packages" release . > Release
  gpg --batch --yes --armor --detach-sign --default-key "$GPG_FPR" -o Release.gpg Release
  gpg --batch --yes --clearsign --default-key "$GPG_FPR" -o InRelease Release
  gpg --armor --export "$GPG_FPR" > key.asc
)
# Scope RPM configuration to this invocation, without overwriting ~/.rpmmacros.
for package in "$RPM"/*.rpm; do
  rpmsign --define "__gpg $(command -v gpg)" --define "_gpg_name $GPG_FPR" \
    --define "__gpg_sign_cmd %{__gpg} gpg --batch --pinentry-mode loopback --no-armor -u %{_gpg_name} --detach-sign -o %{__signature_filename} %{__plaintext_filename}" \
    --addsign "$package"
done
createrepo_c "$RPM"
gpg --batch --yes --armor --detach-sign --default-key "$GPG_FPR" -o "$RPM/repodata/repomd.xml.asc" "$RPM/repodata/repomd.xml"
gpg --armor --export "$GPG_FPR" > "$RPM/key.asc"
cat > "$RPM/actionlay.repo" <<REPO
[actionlay-$CHANNEL]
name=ActionLay ($CHANNEL)
baseurl=https://porech.github.io/actionlay/$CHANNEL/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://porech.github.io/actionlay/$CHANNEL/rpm/key.asc
REPO
