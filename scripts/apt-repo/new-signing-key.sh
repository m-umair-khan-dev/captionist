#!/usr/bin/env bash
# ONE-TIME: creates the key that signs Captionist's APT repository.
#
#   scripts/apt-repo/new-signing-key.sh
#
# Writes:
#   packaging/apt/captionist.asc        public key: commit this (users and the .deb use it)
#   captionist-apt-signing-key.asc      PRIVATE key: store it as the GitHub secret
#                                       APT_SIGNING_KEY, keep an offline backup, then
#                                       delete this file. Never commit it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HOME_TMP=$(mktemp -d)
trap 'rm -rf "$HOME_TMP"' EXIT
export GNUPGHOME="$HOME_TMP"

gpg --batch --pinentry-mode loopback --passphrase '' \
    --quick-gen-key "Captionist APT repository <m-umair-khan-dev@users.noreply.github.com>" rsa4096 sign 0
FPR=$(gpg --list-keys --with-colons | awk -F: '/^fpr/{print $10; exit}')

mkdir -p "$ROOT/packaging/apt"
gpg --armor --export "$FPR" > "$ROOT/packaging/apt/captionist.asc"
umask 077
gpg --armor --export-secret-keys "$FPR" > "$ROOT/captionist-apt-signing-key.asc"

echo "Fingerprint: $FPR"
echo "Public key:  packaging/apt/captionist.asc (commit it)"
echo "Private key: captionist-apt-signing-key.asc"
echo "  1. gh secret set APT_SIGNING_KEY < captionist-apt-signing-key.asc"
echo "  2. Back it up offline (password manager / USB), then: shred -u captionist-apt-signing-key.asc"
