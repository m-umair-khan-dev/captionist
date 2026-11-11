#!/usr/bin/env bash
# Builds sherpa-onnx against ONNX Runtime 1.24.1 in the build container.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if ! docker info >/dev/null 2>&1 && docker --context desktop-linux info >/dev/null 2>&1; then
  export DOCKER_CONTEXT=desktop-linux
fi
docker build -q -t captionist-build:latest "$ROOT/packaging/docker" >/dev/null
exec docker run --rm -u "$(id -u):$(id -g)" -e HOME=/tmp -e JOBS="${JOBS:-2}" -v "$ROOT:$ROOT" -w "$ROOT" \
  captionist-build:latest bash "$ROOT/packaging/sherpa/build-sherpa.sh" "$ROOT"
