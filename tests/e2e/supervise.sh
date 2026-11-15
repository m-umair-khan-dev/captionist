#!/bin/bash
# Stand-in for systemd in the test session: run captionistd and restart it
# when it fails, like captionistd.service (Restart=on-failure, RestartSec=3).
# The service exits on purpose (code 75) to switch accelerators.
while true; do
  "$@"
  code=$?
  [[ $code -eq 0 ]] && exit 0
  echo "supervise: captionistd exited with $code, restarting in 3 s" >&2
  sleep 3
done
