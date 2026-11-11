#!/usr/bin/env bash
# Run cargo inside the Captionist build container (packaging/docker/Dockerfile). The project is mounted at the same
# path as on the host, so built binaries in ./target run directly on the host.
# Usage: scripts/cargo-docker.sh build --release
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Use Docker Desktop if the current context isn't reachable.
if ! docker info >/dev/null 2>&1 && docker --context desktop-linux info >/dev/null 2>&1; then
  export DOCKER_CONTEXT=desktop-linux
fi
IMAGE="${CAPTIONIST_RUST_IMAGE:-captionist-build:latest}"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  docker build -t captionist-build:latest "$ROOT/packaging/docker" >&2
fi
# Local settings (e.g. the Sentry DSN) come from .env, which is never committed.
# In CI the same variables come from repository secrets.
if [[ -f "$ROOT/.env" ]]; then
  set -a; source "$ROOT/.env"; set +a
fi
if [[ ! -f "$ROOT/third_party/sherpa-onnx/lib/libsherpa-onnx-c-api.so" ]]; then
  echo "sherpa-onnx not built yet: run scripts/build-sherpa.sh first" >&2
  exit 1
fi
# Rust package cache. Locally: the "captionist-cargo" Docker volume (kept between
# builds). In CI: a plain folder (CAPTIONIST_CARGO_HOME) that GitHub's cache saves
# between runs, since every CI run starts on a fresh machine.
if [[ -n "${CAPTIONIST_CARGO_HOME:-}" ]]; then
  mkdir -p "$CAPTIONIST_CARGO_HOME"
  CARGO_MOUNT="$CAPTIONIST_CARGO_HOME:/cargo"
else
  docker volume create captionist-cargo >/dev/null
  # New volumes are owned by root; make the crate cache writable for this user.
  docker run --rm -v captionist-cargo:/cargo "$IMAGE" chown "$(id -u):$(id -g)" /cargo
  CARGO_MOUNT="captionist-cargo:/cargo"
fi
exec docker run --rm -u "$(id -u):$(id -g)" \
  -e CARGO_HOME=/cargo -e CARGO_TERM_COLOR=always \
  -e SHERPA_ONNX_LIB_DIR="$ROOT/third_party/sherpa-onnx/lib" \
  -e CAPTIONIST_SENTRY_DSN="${CAPTIONIST_SENTRY_DSN:-}" \
  -v "$CARGO_MOUNT" -v "$ROOT:$ROOT" -w "$ROOT" \
  "$IMAGE" cargo "$@"
