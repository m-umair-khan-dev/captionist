#!/bin/bash
# Runs inside a fresh Ubuntu container (see scripts/compat-test.sh): installs the
# Captionist .deb like a user would and checks it works on that release.
#   /deb/captionist.deb   the package to test
#   /t                    this folder
#   /host-models          optional: downloaded speech models (enables the live-caption check)
#   /host-state           optional: the host's speed measurements
# Exits non-zero if any check fails.
set -u
export DEBIAN_FRONTEND=noninteractive HOME=/root XDG_RUNTIME_DIR=/tmp/rt
FAILED=0
step() { echo; echo "=== $*"; }
fail() { echo "FAIL: $*"; FAILED=1; }

step "System"
grep PRETTY_NAME /etc/os-release
apt-get update -qq >/dev/null

step "Install the .deb (apt resolves the dependencies)"
if apt-get install -y -qq /deb/captionist.deb gnome-shell-extension-ubuntu-dock pipewire wireplumber \
     dbus-x11 dconf-service python3-dbusmock file > /tmp/apt.log 2>&1; then
  echo "OK: captionist $(dpkg-query -W -f='${Version}' captionist)," \
    "gnome-shell $(dpkg-query -W -f='${Version}' gnome-shell)," \
    "glibc $(ldd --version | head -1 | awk '{print $NF}')," \
    "gtk $(dpkg-query -W -f='${Version}' libgtk-4-1)," \
    "pipewire $(dpkg-query -W -f='${Version}' pipewire-bin)"
else
  tail -20 /tmp/apt.log; fail "install"; exit 1
fi

step "Libraries"
for f in $(dpkg -L captionist); do
  [ -f "$f" ] && file -b "$f" | grep -q ELF || continue
  missing=$(ldd "$f" 2>&1 | grep "not found")
  [ -n "$missing" ] && fail "$f: $missing"
done
[ $FAILED = 0 ] && echo "OK: every program and library finds its dependencies"

# GNOME Shell needs a system bus and logind; containers have neither, so use a
# system bus with python-dbusmock's logind (as GNOME's own CI does).
mkdir -p "$XDG_RUNTIME_DIR" /run/dbus /tmp/.X11-unix
chmod 700 "$XDG_RUNTIME_DIR"; chmod 1777 /tmp/.X11-unix
dbus-daemon --system --fork
export DBUS_SYSTEM_BUS_ADDRESS=unix:path=/run/dbus/system_bus_socket
python3 -m dbusmock --template logind --system > /tmp/mock.log 2>&1 &
mkdir -p ~/.local/share/captionist ~/.local/state/captionist
if [ -d /host-models ]; then
  ln -s /host-models ~/.local/share/captionist/models
  cp /host-state/speed.json ~/.local/state/captionist/ 2>/dev/null
fi
sleep 2

dbus-run-session -- bash /t/session-tests.sh || FAILED=1

step "Result"
if [ $FAILED = 0 ]; then echo "ALL CHECKS PASSED"; else echo "SOME CHECKS FAILED"; fi
exit $FAILED
