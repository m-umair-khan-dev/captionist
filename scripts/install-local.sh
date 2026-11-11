#!/usr/bin/env bash
# Installs Captionist for the current user only (no sudo), into ~/.local.
#
#   scripts/install-local.sh                 build (in Docker) + install
#   scripts/install-local.sh --no-build      install already-built binaries
#   scripts/install-local.sh --seed-models DIR
#        also copy already-downloaded recommended models from DIR
#        (e.g. .bench/models) instead of downloading them again
#
# Uninstall: scripts/uninstall-local.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="$HOME/.local"
BINDIR="$PREFIX/bin"
LIBEXECDIR="$PREFIX/lib/captionist"
DATADIR="${XDG_DATA_HOME:-$HOME/.local/share}"
UUID="captionist@captionist.github.io"
BUILD=1
SEED=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) BUILD=0 ;;
    --seed-models) SEED="$2"; shift ;;
    *) echo "unknown option $1" >&2; exit 1 ;;
  esac
  shift
done

if [[ $BUILD == 1 ]]; then
  echo "==> Building (Docker)"
  "$ROOT/scripts/cargo-docker.sh" build --release -p captionist -p captionistd -p captionist-bench
fi

echo "==> Stopping a running caption service"
systemctl --user stop captionistd.service 2>/dev/null || true
pkill -x captionistd 2>/dev/null || true

echo "==> Installing programs"
install -Dm755 "$ROOT/target/release/captionist" "$BINDIR/captionist"
install -Dm755 "$ROOT/target/release/captionistd" "$LIBEXECDIR/captionistd"
install -Dm755 "$ROOT/target/release/captionist-bench" "$LIBEXECDIR/captionist-bench"
# sherpa-onnx + ONNX Runtime (CPU) libraries, found via the binaries' $ORIGIN rpath.
cp -a "$ROOT"/third_party/sherpa-onnx/lib/*.so* "$LIBEXECDIR/"

echo "==> Installing data"
install -Dm644 "$ROOT/data/calibration.wav" "$DATADIR/captionist/calibration.wav"
install -Dm644 "$ROOT/data/silero_vad.onnx" "$DATADIR/captionist/silero_vad.onnx"
install -Dm644 "$ROOT/data/icons/io.github.captionist.Captionist.svg" \
  "$DATADIR/icons/hicolor/scalable/apps/io.github.captionist.Captionist.svg"
mkdir -p "$DATADIR/applications"
sed "s|@BINDIR@|$BINDIR|g" "$ROOT/data/io.github.captionist.Captionist.desktop.in" \
  > "$DATADIR/applications/io.github.captionist.Captionist.desktop"

echo "==> Installing settings schema"
install -Dm644 "$ROOT/data/io.github.captionist.Captionist.gschema.xml" \
  "$DATADIR/glib-2.0/schemas/io.github.captionist.Captionist.gschema.xml"
glib-compile-schemas "$DATADIR/glib-2.0/schemas"

echo "==> Installing caption service (D-Bus + systemd user unit)"
mkdir -p "$DATADIR/dbus-1/services" "$HOME/.config/systemd/user"
sed "s|@LIBEXECDIR@|$LIBEXECDIR|g" "$ROOT/data/io.github.captionist.Daemon.service.in" \
  > "$DATADIR/dbus-1/services/io.github.captionist.Daemon.service"
sed "s|@LIBEXECDIR@|$LIBEXECDIR|g" "$ROOT/data/captionistd.service.in" \
  > "$HOME/.config/systemd/user/captionistd.service"
systemctl --user daemon-reload
gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
  --method org.freedesktop.DBus.ReloadConfig >/dev/null 2>&1 || true

echo "==> Installing GNOME Shell extension"
EXT="$DATADIR/gnome-shell/extensions/$UUID"
rm -rf "$EXT"
mkdir -p "$EXT/schemas"
cp "$ROOT"/extension/{metadata.json,extension.js,overlay.js,controls.js,stylesheet.css} "$EXT/"
cp -r "$ROOT/extension/icons" "$EXT/"
# The extension shows the same version as the app (single source: Cargo.toml).
VERSION=$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)
sed -i "s/\"version-name\": \"[^\"]*\"/\"version-name\": \"$VERSION\"/" "$EXT/metadata.json"
cp "$ROOT/data/io.github.captionist.Captionist.gschema.xml" "$EXT/schemas/"
glib-compile-schemas "$EXT/schemas"

if [[ -n "$SEED" ]]; then
  echo "==> Copying recommended models from $SEED"
  MODELS="$DATADIR/captionist/models"
  mkdir -p "$MODELS"
  for d in sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06 sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8; do
    if [[ -d "$SEED/$d" && ! -d "$MODELS/$d" ]]; then
      cp -al "$SEED/$d" "$MODELS/$d" 2>/dev/null || cp -a "$SEED/$d" "$MODELS/$d"
      rm -rf "$MODELS/$d/test_wavs"
    fi
  done
fi

update-desktop-database "$DATADIR/applications" >/dev/null 2>&1 || true
gtk-update-icon-cache -q -t "$DATADIR/icons/hicolor" >/dev/null 2>&1 || true

# Enable the extension (takes effect once GNOME Shell has loaded it).
if command -v gnome-extensions >/dev/null; then
  gnome-extensions enable "$UUID" 2>/dev/null || {
    cur=$(gsettings get org.gnome.shell enabled-extensions)
    if [[ "$cur" != *"$UUID"* ]]; then
      if [[ "$cur" == "@as []" || "$cur" == "[]" ]]; then new="['$UUID']"; else new="${cur%]}, '$UUID']"; fi
      gsettings set org.gnome.shell enabled-extensions "$new"
    fi
  }
fi

echo
echo "Captionist installed."
if gnome-extensions info "$UUID" 2>/dev/null | grep -qE 'State: (ACTIVE|ENABLED)'; then
  echo "The GNOME extension is active."
else
  echo "Log out and back in once so GNOME Shell loads the extension (Wayland can't reload it live)."
fi
echo "Open 'Captionist' from the app grid, or press Super+Ctrl+L after logging back in."
