#!/bin/bash
# Run a command inside the private test session started by session.sh.
R=${E2E_ROOT:-$HOME/.cache/captionist-e2e}
[[ -s $R/env ]] || { echo "no test session; run tests/e2e/session.sh first" >&2; exit 1; }
args=()
while IFS= read -r l; do [[ $l == *=* ]] && args+=("$l"); done < <(systemctl --user show-environment)
exec env -i "${args[@]}" bash -c 'source "$0/env"; unset DISPLAY; exec "$@"' "$R" "$@"
