#!/usr/bin/env bash
# Adds .deb packages to a signed APT repository and regenerates its indexes.
#
#   scripts/apt-repo/publish.sh <repo-dir> <package.deb>...
#
# Layout (served from GitHub Pages at https://m-umair-khan-dev.github.io/captionist/apt):
#   <repo-dir>/pool/main/c/captionist/*.deb
#   <repo-dir>/dists/stable/main/binary-amd64/Packages{,.gz}
#   <repo-dir>/dists/stable/{Release,Release.gpg,InRelease}
#
# Needs: apt-utils (apt-ftparchive) and the signing key imported into gpg.
set -euo pipefail
REPO="$1"; shift
[[ $# -gt 0 ]] || { echo "usage: $0 <repo-dir> <package.deb>..." >&2; exit 1; }

POOL="$REPO/pool/main/c/captionist"
DIST="$REPO/dists/stable"
mkdir -p "$POOL" "$DIST/main/binary-amd64"
cp -n "$@" "$POOL/" 2>/dev/null || cp "$@" "$POOL/"

# Keep the 5 newest versions so users can still downgrade after a bad release.
ls -1t "$POOL"/captionist_*.deb | tail -n +6 | xargs -r rm -f

(cd "$REPO" && apt-ftparchive --arch amd64 packages pool > dists/stable/main/binary-amd64/Packages)
gzip -9kf "$DIST/main/binary-amd64/Packages"

apt-ftparchive \
  -o APT::FTPArchive::Release::Origin="Captionist" \
  -o APT::FTPArchive::Release::Label="Captionist" \
  -o APT::FTPArchive::Release::Suite="stable" \
  -o APT::FTPArchive::Release::Codename="stable" \
  -o APT::FTPArchive::Release::Architectures="amd64" \
  -o APT::FTPArchive::Release::Components="main" \
  -o APT::FTPArchive::Release::Description="Captionist live captions for Linux" \
  release "$DIST" > "$DIST/Release.new"
mv "$DIST/Release.new" "$DIST/Release"

KEY=$(gpg --list-secret-keys --with-colons | awk -F: '/^sec/{print $5; exit}')
[[ -n "$KEY" ]] || { echo "no signing key imported into gpg" >&2; exit 1; }
gpg --batch --yes --default-key "$KEY" --armor --detach-sign -o "$DIST/Release.gpg" "$DIST/Release"
gpg --batch --yes --default-key "$KEY" --clearsign -o "$DIST/InRelease" "$DIST/Release"
echo "Published $(ls "$POOL" | wc -l) package(s) to $REPO"
