#!/usr/bin/env bash
# Installs a Captionist .deb in a fresh Ubuntu container and checks it works:
# libraries, the caption service, live captions (if speech models are
# downloaded on this PC) and the extension inside a real GNOME Shell (no screen).
#
#   scripts/compat-test.sh [ubuntu-version ...]     (default: 24.04 26.04 26.10)
#   DEB=dist/captionist_X_amd64.deb scripts/compat-test.sh 24.04
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if ! docker info >/dev/null 2>&1 && docker --context desktop-linux info >/dev/null 2>&1; then
  export DOCKER_CONTEXT=desktop-linux
fi
VERSION=$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)
DEB="$(realpath "${DEB:-$ROOT/dist/captionist_${VERSION}_amd64.deb}")"
[[ -f "$DEB" ]] || { echo "no .deb at $DEB (run scripts/build-deb.sh first)" >&2; exit 1; }
MOUNTS=(-v "$ROOT/packaging/compat-test:/t:ro" -v "$DEB:/deb/captionist.deb:ro")
MODELS="${XDG_DATA_HOME:-$HOME/.local/share}/captionist/models"
STATE="${XDG_STATE_HOME:-$HOME/.local/state}/captionist"
if [[ -d "$MODELS" && -n "$(ls -A "$MODELS" 2>/dev/null)" ]]; then
  MOUNTS+=(-v "$MODELS:/host-models:ro")
  [[ -d "$STATE" ]] && MOUNTS+=(-v "$STATE:/host-state:ro")
fi
status=0
for v in "${@:-24.04 26.04 26.10}"; do
  for release in $v; do
    echo "##### Ubuntu $release"
    # pipefail: the pipeline fails if the test inside the container fails.
    if ! docker run --rm "${MOUNTS[@]}" "ubuntu:$release" bash /t/test-distro.sh 2>&1 \
      | grep -vE "dbus-daemon\[|SpiRegistry|xdg-desktop-portal|GVFS|goa-daemon|fuse|evolution|calendar-server|Nautilus|^$"; then
      status=1
    fi
  done
done
exit $status
