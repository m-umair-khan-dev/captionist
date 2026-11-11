#!/usr/bin/env bash
# Builds sherpa-onnx (C API, shared) against ONNX Runtime $ORT_VER so the
# runtime can be swapped at load time: Microsoft's CPU build by default, or
# Intel's OpenVINO build of the same version for Intel GPU/NPU acceleration.
# Runs inside the captionist-build container (see scripts/build-sherpa.sh).
# Output: third_party/sherpa-onnx/lib/{libsherpa-onnx-c-api.so,libonnxruntime.so*}
set -euo pipefail
ROOT="$1"
SHERPA_VER=2.0.1
ORT_VER=1.24.1
OUT="$ROOT/third_party/sherpa-onnx"
WORK="$ROOT/third_party/.build"
mkdir -p "$WORK" "$OUT/lib"
cd "$WORK"

if [[ ! -d onnxruntime-linux-x64-$ORT_VER ]]; then
  curl -fsSL "https://github.com/microsoft/onnxruntime/releases/download/v$ORT_VER/onnxruntime-linux-x64-$ORT_VER.tgz" | tar xz
fi
if [[ ! -d sherpa-onnx-src ]]; then
  git clone -q --depth 1 -b "v$SHERPA_VER" https://github.com/k2-fsa/sherpa-onnx.git sherpa-onnx-src
fi

export SHERPA_ONNXRUNTIME_LIB_DIR="$WORK/onnxruntime-linux-x64-$ORT_VER/lib"
export SHERPA_ONNXRUNTIME_INCLUDE_DIR="$WORK/onnxruntime-linux-x64-$ORT_VER/include"
cmake -S sherpa-onnx-src -B build -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS=ON \
  -DSHERPA_ONNX_ENABLE_PYTHON=OFF -DSHERPA_ONNX_ENABLE_TESTS=OFF -DSHERPA_ONNX_ENABLE_CHECK=OFF \
  -DSHERPA_ONNX_ENABLE_PORTAUDIO=OFF -DSHERPA_ONNX_ENABLE_WEBSOCKET=OFF -DSHERPA_ONNX_ENABLE_BINARY=OFF \
  -DSHERPA_ONNX_ENABLE_C_API=ON -DSHERPA_ONNX_ENABLE_GPU=OFF -DSHERPA_ONNX_ENABLE_JNI=OFF
cmake --build build -j "${JOBS:-2}" --target sherpa-onnx-c-api

cp -L build/lib/libsherpa-onnx-c-api.so "$OUT/lib/"
cp -a "$SHERPA_ONNXRUNTIME_LIB_DIR"/libonnxruntime.so* "$OUT/lib/"
echo "$SHERPA_VER ort$ORT_VER" > "$OUT/VERSION"
ls -la "$OUT/lib"
