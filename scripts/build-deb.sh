#!/usr/bin/env bash
# Builds dist/captionist_<version>_amd64.deb (system-wide install under /usr).
#   scripts/build-deb.sh [--no-build]
# Install with:  sudo apt install ./dist/captionist_<version>_amd64.deb
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION=$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)
UUID="captionist@m-umair-khan-dev.github.io"

[[ "${1:-}" == "--no-build" ]] || "$ROOT/scripts/cargo-docker.sh" build --release -p captionist -p captionistd

STAGE="$ROOT/target/deb-stage"
rm -rf "$STAGE"; mkdir -p "$STAGE"
trap 'rm -rf "$STAGE"' EXIT
P="$STAGE/pkg"

install -Dm755 "$ROOT/target/release/captionist" "$P/usr/bin/captionist"
install -Dm755 "$ROOT/target/release/captionistd" "$P/usr/libexec/captionist/captionistd"
cp -a "$ROOT"/third_party/sherpa-onnx/lib/*.so* "$P/usr/libexec/captionist/"
install -Dm644 "$ROOT/data/calibration.wav" "$P/usr/share/captionist/calibration.wav"
install -Dm644 "$ROOT/data/silero_vad.onnx" "$P/usr/share/captionist/silero_vad.onnx"
install -Dm644 "$ROOT/data/io.github.m_umair_khan_dev.Captionist.gschema.xml" "$P/usr/share/glib-2.0/schemas/io.github.m_umair_khan_dev.Captionist.gschema.xml"
install -Dm644 "$ROOT/data/icons/io.github.m_umair_khan_dev.Captionist.svg" "$P/usr/share/icons/hicolor/scalable/apps/io.github.m_umair_khan_dev.Captionist.svg"
install -Dm644 "$ROOT/data/icons/io.github.m_umair_khan_dev.Captionist-symbolic.svg" "$P/usr/share/icons/hicolor/symbolic/apps/io.github.m_umair_khan_dev.Captionist-symbolic.svg"
mkdir -p "$P/usr/share/applications" "$P/usr/share/dbus-1/services" "$P/usr/lib/systemd/user"
sed "s|@BINDIR@|/usr/bin|g" "$ROOT/data/io.github.m_umair_khan_dev.Captionist.desktop.in" > "$P/usr/share/applications/io.github.m_umair_khan_dev.Captionist.desktop"
sed "s|@LIBEXECDIR@|/usr/libexec/captionist|g" "$ROOT/data/io.github.m_umair_khan_dev.Captionist.Daemon.service.in" > "$P/usr/share/dbus-1/services/io.github.m_umair_khan_dev.Captionist.Daemon.service"
sed "s|@LIBEXECDIR@|/usr/libexec/captionist|g" "$ROOT/data/captionistd.service.in" > "$P/usr/lib/systemd/user/captionistd.service"

EXT="$P/usr/share/gnome-shell/extensions/$UUID"
mkdir -p "$EXT/schemas"
cp "$ROOT"/extension/{metadata.json,extension.js,overlay.js,controls.js,stylesheet.css} "$EXT/"
cp -r "$ROOT/extension/icons" "$EXT/"
# The extension shows the same version as the app (single source: Cargo.toml).
sed -i "s/\"version-name\": \"[^\"]*\"/\"version-name\": \"$VERSION\"/" "$EXT/metadata.json"
cp "$ROOT/data/io.github.m_umair_khan_dev.Captionist.gschema.xml" "$EXT/schemas/"
glib-compile-schemas "$EXT/schemas"

mkdir -p "$P/DEBIAN"

# Updates: once the repository signing key exists (scripts/apt-repo/new-signing-key.sh),
# the package adds Captionist's APT repository, so installing the .deb once means
# later versions arrive with normal system updates, including unattended-upgrades.
APT_KEY="$ROOT/packaging/apt/captionist.asc"
if [[ -f "$APT_KEY" ]]; then
  install -Dm644 "$APT_KEY" "$P/usr/share/keyrings/captionist.asc"
  install -Dm644 /dev/stdin "$P/etc/apt/sources.list.d/captionist.sources" <<'EOF'
# Captionist updates (https://github.com/m-umair-khan-dev/captionist)
Types: deb
URIs: https://m-umair-khan-dev.github.io/captionist/apt
Suites: stable
Components: main
Architectures: amd64
Signed-By: /usr/share/keyrings/captionist.asc
EOF
  install -Dm644 /dev/stdin "$P/etc/apt/apt.conf.d/52unattended-upgrades-captionist" <<'EOF'
// Let unattended-upgrades install Captionist updates (it only considers
// Ubuntu's own archives by default).
Unattended-Upgrade::Origins-Pattern {
        "origin=Captionist,suite=stable";
};
EOF
  printf '%s\n' /etc/apt/sources.list.d/captionist.sources /etc/apt/apt.conf.d/52unattended-upgrades-captionist > "$P/DEBIAN/conffiles"
else
  echo "note: packaging/apt/captionist.asc not found; building without the update repository" >&2
fi
SIZE=$(du -sk "$P/usr" | cut -f1)
cat > "$P/DEBIAN/control" <<EOF
Package: captionist
Version: $VERSION
Section: gnome
Priority: optional
Architecture: amd64
Installed-Size: $SIZE
Depends: libc6 (>= 2.39), libstdc++6, libgtk-4-1 (>= 4.20), libadwaita-1-0 (>= 1.8), pipewire-bin, gnome-shell (>= 50)
Conflicts: hearline
Replaces: hearline
Maintainer: m-umair-khan-dev <m-umair-khan-dev@users.noreply.github.com>
Homepage: https://github.com/m-umair-khan-dev/captionist
Description: Real-time, private live captions for the Linux desktop
 Captionist captions anything you hear (videos, calls, meetings) or your
 microphone, fully on-device. A fast streaming model shows text as people
 speak; a more accurate model corrects each sentence when it ends.
 Includes a GNOME Shell extension (caption overlay, top-bar icon, Quick
 Settings toggle) and a settings window. Speech models are downloaded on
 demand after checking what this PC can run.
EOF
cat > "$P/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "configure" ]; then
  glib-compile-schemas /usr/share/glib-2.0/schemas >/dev/null 2>&1 || true
  gtk-update-icon-cache -q /usr/share/icons/hicolor >/dev/null 2>&1 || true
  update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true
fi
EOF
cat > "$P/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
glib-compile-schemas /usr/share/glib-2.0/schemas >/dev/null 2>&1 || true
EOF
chmod 755 "$P/DEBIAN/postinst" "$P/DEBIAN/postrm"

mkdir -p "$ROOT/dist"
OUT="$ROOT/dist/captionist_${VERSION}_amd64.deb"
dpkg-deb --root-owner-group --build "$P" "$OUT" >/dev/null
echo "Built $OUT ($(du -h "$OUT" | cut -f1))"
