#!/usr/bin/env bash
# Removes a per-user Captionist install. Downloaded models are kept unless --purge.
set -euo pipefail
DATADIR="${XDG_DATA_HOME:-$HOME/.local/share}"
UUID="captionist@captionist.github.io"

gnome-extensions disable "$UUID" 2>/dev/null || true
systemctl --user stop captionistd.service 2>/dev/null || true
pkill -x captionistd 2>/dev/null || true

rm -f "$HOME/.local/bin/captionist"
rm -rf "$HOME/.local/lib/captionist"
rm -rf "$DATADIR/gnome-shell/extensions/$UUID"
rm -f "$DATADIR/applications/io.github.captionist.Captionist.desktop"
rm -f "$DATADIR/icons/hicolor/scalable/apps/io.github.captionist.Captionist.svg"
rm -f "$DATADIR/dbus-1/services/io.github.captionist.Daemon.service" "$DATADIR/dbus-1/services/io.github.captionist.Captionist.service"
rm -f "$HOME/.config/systemd/user/captionistd.service"
rm -f "$DATADIR/glib-2.0/schemas/io.github.captionist.Captionist.gschema.xml"
glib-compile-schemas "$DATADIR/glib-2.0/schemas" 2>/dev/null || true
rm -f "$DATADIR/captionist/calibration.wav" "$DATADIR/captionist/silero_vad.onnx"
systemctl --user daemon-reload

if [[ "${1:-}" == "--purge" ]]; then
  rm -rf "$DATADIR/captionist" "${XDG_STATE_HOME:-$HOME/.local/state}/captionist"
  dconf reset -f /io/github/captionist/Captionist/ 2>/dev/null || true
  echo "Captionist removed, including models and settings."
else
  echo "Captionist removed. Models kept in $DATADIR/captionist/models (use --purge to delete)."
fi
