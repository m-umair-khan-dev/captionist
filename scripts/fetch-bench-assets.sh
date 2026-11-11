#!/usr/bin/env bash
# Downloads candidate models + LibriSpeech test audio for captionist-bench.
# Usage: scripts/fetch-bench-assets.sh [dest_dir]   (default: .bench)
set -euo pipefail
DEST="${1:-$(dirname "$0")/../.bench}"
mkdir -p "$DEST/models" "$DEST/data"
BASE=https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models

MODELS=(
  # Pass 1 (streaming)
  sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06
  sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-160ms-int8-2026-06-11
  sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11
  # Pass 2 (refine)
  sherpa-onnx-moonshine-tiny-en-quantized-2026-02-27
  sherpa-onnx-moonshine-base-en-quantized-2026-02-27
  sherpa-onnx-nemo-parakeet_tdt_transducer_110m-en-36000-int8
  sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8
)

for m in "${MODELS[@]}"; do
  if [[ -d "$DEST/models/$m" ]]; then echo "have $m"; continue; fi
  echo "fetch $m"
  # Download fully (with retries) before extracting, so a failed download
  # never leaves a half-extracted model behind.
  curl -fsSL --retry 5 --retry-all-errors -o "$DEST/models/$m.tar.bz2.part" "$BASE/$m.tar.bz2"
  tar -xjf "$DEST/models/$m.tar.bz2.part" -C "$DEST/models"
  rm -f "$DEST/models/$m.tar.bz2.part"
done
[[ -f "$DEST/models/silero_vad.onnx" ]] || curl -fsSL -o "$DEST/models/silero_vad.onnx" "$BASE/silero_vad.onnx"

# LibriSpeech test sets -> 16 kHz mono WAV + reference transcripts
for set in test-clean test-other; do
  out="$DEST/data/$set"
  if [[ -f "$out/refs.tsv" ]]; then echo "have $set"; continue; fi
  mkdir -p "$out/wav"
  tmp=$(mktemp -d)
  echo "fetch LibriSpeech $set"
  curl -fsSL --retry 5 --retry-all-errors "https://www.openslr.org/resources/12/$set.tar.gz" | tar -xz -C "$tmp"
  # Take the first 3 utterances of each chapter -> ~120 utterances, spread across speakers.
  : > "$out/refs.tsv"
  find "$tmp" -name '*.trans.txt' | sort | while read -r t; do
    head -n 3 "$t" | while read -r id text; do
      flac=$(dirname "$t")/$id.flac
      ffmpeg -loglevel error -nostdin -y -i "$flac" -ar 16000 -ac 1 -c:a pcm_s16le "$out/wav/$id.wav"
      printf '%s\t%s\n' "$id" "$text" >> "$out/refs.tsv"
    done
  done
  rm -rf "$tmp"
done
echo "done: $DEST"
