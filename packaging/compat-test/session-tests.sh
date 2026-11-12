#!/bin/bash
# The checks that need a desktop session bus (run by test-distro.sh).
set -u
FAILED=0
step() { echo; echo "=== $*"; }
fail() { echo "FAIL: $*"; FAILED=1; }
NAME=io.github.m_umair_khan_dev.Captionist
B="--session --dest $NAME.Daemon --object-path /io/github/m_umair_khan_dev/Captionist/Daemon"
M="$B --method $NAME.Daemon1"

pipewire >/dev/null 2>&1 & sleep 1
wireplumber >/dev/null 2>&1 & sleep 2
# A virtual speaker, so "speakers" capture has something to record.
pw-cli create-node adapter "{ factory.name=support.null-audio-sink node.name=testspeaker media.class=Audio/Sink object.linger=true audio.position=[FL FR] }" >/dev/null
sleep 2

step "Caption service over D-Bus"
ping=$(gdbus call $M.Ping 2>&1)
echo "Ping: $ping"
[[ "$ping" == *"$(dpkg-query -W -f='${Version}' captionist)"* ]] || fail "service did not answer Ping"
gdbus call $M.ListModels recommended | grep -q '"rows"' && echo "OK: ListModels" || fail "ListModels"
gdbus call $M.DiagnosticsReport | grep -oE "System: [^\\]*" | head -1

step "Live captions from the speakers"
if [ -L ~/.local/share/captionist/models ]; then
  timeout 75 gdbus monitor $B > /tmp/sig.txt 2>&1 &
  gsettings set $NAME enabled true
  for i in $(seq 1 40); do gdbus call $M.Status | grep -q listening && break; sleep 1; done
  echo "status: $(gdbus call $M.Status)"
  timeout 20 pw-play /usr/share/captionist/calibration.wav
  sleep 6
  partial=$(grep -c "\.Partial " /tmp/sig.txt); final=$(grep -c "\.Final " /tmp/sig.txt)
  echo "Partial signals: $partial, Final signals: $final"
  grep "\.Final " /tmp/sig.txt | head -2 | sed "s/.*Final (/Final (/" | cut -c1-150
  { [ "$partial" -gt 0 ] && [ "$final" -gt 0 ]; } || fail "no captions"
  gsettings set $NAME enabled false
else
  echo "skipped: no speech models available (mount them at /host-models)"
fi

step "GNOME Shell with Captionist + Ubuntu Dock (no screen)"
gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "['captionist@m-umair-khan-dev.github.io', 'ubuntu-dock@ubuntu.com', 'captest@local']"
mkdir -p ~/.local/share/gnome-shell/extensions && cp -r /t/captest@local ~/.local/share/gnome-shell/extensions/
gnome-shell --headless --virtual-monitor 1920x1080 --no-x11 > /tmp/shell.log 2>&1 &
SHELL_PID=$!
for i in $(seq 1 90); do grep -q "CAPTEST done" /tmp/shell.log && break; sleep 1; done
grep -o "CAPTEST.*" /tmp/shell.log
if ! grep -q "CAPTEST done" /tmp/shell.log; then
  echo "last shell log lines:"; grep -vE "dbus-daemon|portal" /tmp/shell.log | tail -25
  fail "shell test did not finish"
fi
grep -q "CAPTEST extension-state 1 no-error" /tmp/shell.log || fail "extension not enabled cleanly"
grep -qE "CAPTEST.*(MISSING|OVERLAPS-BOX|FAIL)" /tmp/shell.log && fail "see CAPTEST lines above"
errors=$(grep -E "JS ERROR|TypeError|ReferenceError" /tmp/shell.log | grep -iE "captionist|captest" | head -5)
[ -n "$errors" ] && { echo "$errors"; fail "JavaScript errors from the extension"; }
kill $SHELL_PID 2>/dev/null
exit $FAILED
