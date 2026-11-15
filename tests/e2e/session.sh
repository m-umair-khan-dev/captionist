#!/bin/bash
# Start an isolated headless GNOME Shell for Captionist UI tests.
#
# The session has its own D-Bus, settings (dconf), data, state, cache and
# runtime dir, so tests never touch the desktop you're using. It shares only
# your PipeWire (audio and the screen cast used for screenshots). Models are
# a hard-linked copy of ~/.local/share/captionist: no extra disk space, and
# deleting one in a test leaves yours alone.
#
# Usage: tests/e2e/session.sh [path/to/captionistd] [path/to/extension-dir]
#        tests/e2e/session.sh --stop
# Then run commands inside it with tests/e2e/in.sh <command>.
set -euo pipefail
ROOT=${E2E_ROOT:-$HOME/.cache/captionist-e2e}
REAL_RUNTIME=/run/user/$(id -u)
UUID=captionist@m-umair-khan-dev.github.io

if [[ ${1:-} == --stop ]]; then
  if [[ -s $ROOT/env ]]; then
    pid=$(sed -n 's/^export SHELL_PID=//p' "$ROOT/env")
    [[ -n $pid ]] && kill "$pid" 2>/dev/null || true
    rm -f "$ROOT/env"
  fi
  exit 0
fi

if [[ ${1:-} != --inner ]]; then
  DAEMON=${1:-}
  EXTENSION=${2:-}
  [[ -s $ROOT/env ]] && "$0" --stop && sleep 2
  mkdir -p "$ROOT"/{config,data,state,cache}
  RUN=$ROOT/run-$(date +%s)
  mkdir -m 700 "$RUN"
  rm -f "$ROOT/env"
  if [[ ! -d $ROOT/data/captionist && -d $HOME/.local/share/captionist ]]; then
    cp -al "$HOME/.local/share/captionist" "$ROOT/data/captionist"
  fi
  # Use a dev build of the daemon: D-Bus activation in the session starts it,
  # under supervise.sh, which restarts it like systemd does in a real session.
  if [[ -n $DAEMON ]]; then
    mkdir -p "$ROOT/data/dbus-1/services"
    printf '[D-BUS Service]\nName=io.github.m_umair_khan_dev.Captionist.Daemon\nExec=%s %s\n' \
      "$(realpath "$(dirname "$0")/supervise.sh")" "$(realpath "$DAEMON")" \
      >"$ROOT/data/dbus-1/services/io.github.m_umair_khan_dev.Captionist.Daemon.service"
  else
    rm -f "$ROOT/data/dbus-1/services/io.github.m_umair_khan_dev.Captionist.Daemon.service"
  fi
  # Use a dev copy of the extension: a user extension wins over the system one.
  EXT_DIR=$ROOT/data/gnome-shell/extensions/$UUID
  if [[ -n $EXTENSION ]]; then
    mkdir -p "$EXT_DIR"
    cp "$EXTENSION"/{metadata.json,extension.js,overlay.js,controls.js,stylesheet.css} "$EXT_DIR/"
    cp -r "$EXTENSION/icons" "$EXT_DIR/"
  elif [[ -d $EXT_DIR ]]; then
    mv "$EXT_DIR" "$EXT_DIR.off-$(date +%s)"
  fi
  # Start from the desktop session's environment, not the caller's (VS Code's
  # snap, for one, leaks GSETTINGS_SCHEMA_DIR, GIO_MODULE_DIR, GTK_PATH...).
  args=()
  while IFS= read -r l; do [[ $l == *=* ]] && args+=("$l"); done < <(systemctl --user show-environment)
  # Everything is set BEFORE the private bus starts, so every service it
  # activates (dconf, captionistd, at-spi) inherits the private dirs.
  env -i "${args[@]}" E2E_ROOT="$ROOT" \
    XDG_CONFIG_HOME="$ROOT/config" XDG_DATA_HOME="$ROOT/data" XDG_STATE_HOME="$ROOT/state" \
    XDG_CACHE_HOME="$ROOT/cache" XDG_RUNTIME_DIR="$RUN" PIPEWIRE_RUNTIME_DIR="$REAL_RUNTIME" \
    PULSE_SERVER="unix:$REAL_RUNTIME/pulse/native" WAYLAND_DISPLAY=captionist-e2e \
    GDK_BACKEND=wayland DISPLAY= DBUS_SESSION_BUS_ADDRESS= \
    setsid dbus-run-session -- "$0" --inner >"$ROOT/session.log" 2>&1 &
  for _ in $(seq 60); do [[ -s $ROOT/env ]] && break; sleep 0.5; done
  [[ -s $ROOT/env ]] || { echo "session failed to start; see $ROOT/session.log" >&2; exit 1; }
  echo "test session running (env in $ROOT/env)"
  exit 0
fi

# ---- inside the private D-Bus session ----
unset DISPLAY
# Refuse to touch settings unless they really land in the private store.
gsettings set org.gnome.desktop.interface enable-animations false
[[ -f $XDG_CONFIG_HOME/dconf/user ]] || { echo "dconf not isolated; aborting" >&2; exit 1; }
gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "['captionist@m-umair-khan-dev.github.io']"
gsettings set org.gnome.desktop.interface toolkit-accessibility true

gnome-shell --headless --virtual-monitor 1600x1000 --wayland-display captionist-e2e --no-x11 \
  >"$ROOT/shell.log" 2>&1 &
SHELL_PID=$!
for _ in $(seq 60); do [[ -S $XDG_RUNTIME_DIR/captionist-e2e ]] && break; sleep 0.5; done
# The a11y bus's own activation goes through systemd, which can't see this
# session; start the registry directly once the bus exists.
for _ in $(seq 20); do
  gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
    --method org.a11y.Bus.GetAddress >/dev/null 2>&1 && break
  sleep 0.5
done
/usr/libexec/at-spi2-registryd >"$ROOT/registryd.log" 2>&1 &

env | grep -E '^(DBUS_SESSION_BUS_ADDRESS|XDG_[A-Z_]+|PIPEWIRE_RUNTIME_DIR|PULSE_SERVER|WAYLAND_DISPLAY|GDK_BACKEND)=' \
  | sed 's/^/export /' >"$ROOT/env.tmp"
echo "export SHELL_PID=$SHELL_PID" >>"$ROOT/env.tmp"
mv "$ROOT/env.tmp" "$ROOT/env"
wait "$SHELL_PID"
