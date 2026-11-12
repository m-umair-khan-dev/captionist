# Developing Captionist

How Captionist is built, tested, packaged and released.

## Layout

| Path | What |
|---|---|
| `crates/captionist` | Settings window (GTK4 / libadwaita) |
| `crates/captionistd` | Caption service, D-Bus `io.github.m_umair_khan_dev.Captionist.Daemon`, started on demand via a systemd user unit |
| `crates/captionist-engine` | Capture (`pw-record`), Silero VAD, two-pass recognition, calibration, downloads, acceleration packs |
| `crates/captionist-core` | Hardware probe, model catalogue, recommendation and acceleration advice |
| `crates/captionist-bench` | `captionist-bench` CLI: speed and accuracy tests |
| `extension/` | GNOME Shell 51 extension `captionist@m-umair-khan-dev.github.io` (caption box, top-bar menu, Quick Settings) |
| `data/models.toml` | Model catalogue (add a model = add an entry) |
| `scripts/` | Docker cargo wrapper, sherpa-onnx build, packaging, test-asset download |

## Build (Docker, no local Rust needed)

```sh
scripts/build-sherpa.sh                  # once, ~20 min: sherpa-onnx 2.0.1 against ONNX Runtime 1.24.1 → third_party/
scripts/cargo-docker.sh build --release  # Rust workspace → target/release/
scripts/cargo-docker.sh test --release
```

`cargo-docker.sh` runs `cargo` in the `captionist-build` image ([packaging/docker/Dockerfile](../packaging/docker/Dockerfile): Ubuntu 24.04 LTS with GTK/libadwaita dev files and the Rust toolchain; the oldest supported release, so the same build runs on newer Ubuntu), with the project mounted at the same path and a `captionist-cargo` volume for the crate cache. Binaries in `target/release/` run directly on the host.

sherpa-onnx and ONNX Runtime are **shared libraries** installed next to `captionistd` (found via `$ORIGIN`). That lets a vendor build of the same ONNX Runtime version (Intel OpenVINO, NVIDIA CUDA) be swapped in at load time for hardware acceleration.

## Install from source

```sh
scripts/build-deb.sh                # → dist/captionist_<ver>_amd64.deb; sudo apt install ./dist/captionist_*.deb
scripts/install-local.sh            # or a per-user install into ~/.local (no sudo); log out/in once
scripts/uninstall-local.sh [--purge]
```

## Supported Ubuntu releases

One `.deb` supports Ubuntu 24.04 LTS (GNOME 46), 26.04 LTS (GNOME 50) and 26.10 (GNOME 51). It's built on the oldest one, and code that differs between releases checks what's available at run time instead of assuming the newest:

- **PipeWire:** `pw-record --raw` exists only in newer PipeWire. PipeWire 1.0 (24.04) rejects it but already writes plain samples to stdout ([capture.rs](../crates/captionist-engine/src/capture.rs)).
- **Pointer shape over the caption box:** GNOME 48+ sets it per actor (`set_cursor_type`); GNOME 46/47 only have the global cursor ([overlay.js](../extension/overlay.js)).
- **Settings window:** built against GTK 4.14 / libadwaita 1.5 APIs only, which newer GTK runs unchanged.

To check a `.deb` on each release (CI runs the same test on every PR):

```sh
scripts/compat-test.sh              # Ubuntu 24.04, 26.04 and 26.10
scripts/compat-test.sh 24.04        # just one
```

Each run installs the package in a fresh container and checks the libraries, the caption service over D-Bus, live captions from a virtual speaker (when speech models are downloaded on this PC; CI skips this) and the extension inside a real GNOME Shell with the Ubuntu Dock: top-bar icon, Quick Settings toggle, caption box, pointer shape and the box staying clear of the dock.

## Speed and accuracy tests

```sh
scripts/fetch-bench-assets.sh                       # ~3.5 GB: candidate models + LibriSpeech test audio → .bench/
target/release/captionist-bench probe               # what this PC has
target/release/captionist-bench speed               # calibration → estimates + recommendation + sorted list
target/release/captionist-bench speed --all         # time every downloaded model instead of estimating
target/release/captionist-bench eval -o .bench/results/eval.json   # full accuracy + speed table
target/release/captionist-bench assess -m parakeet-tdt-0.6b-v2 -r .bench/results/eval.json
```

Run speed tests on the host, not inside Docker: Docker Desktop's VM has fewer resources and no GPU.

## Hardware acceleration notes

Settings → Models → Acceleration scans for NVIDIA GPUs (CUDA), Intel graphics and Intel NPUs (OpenVINO), advises, and *Set up* downloads the matching ONNX Runtime 1.24.1 pack (checksum-verified), restarts the service on it and measures it against the processor. Accelerator tests run in a separate process, and a model only runs on an accelerator after passing there, so an unsupported model can't crash the service. AMD GPUs aren't supported by sherpa-onnx yet.

Measured on an Intel N150 (2026-11-10): Parakeet TDT 0.6B v2 runs at RTF 0.16 on the processor. Intel's OpenVINO runtime can't run the int8 models (it crashes with "Output names mismatch", reported upstream: [bug-report-openvino-ep-int8.md](bug-report-openvino-ep-int8.md)), and the full-precision Parakeet 110M ran about 3× slower on the built-in graphics than on the processor.

## Model licences

Models are downloaded on demand and are not part of Captionist's source. NVIDIA Parakeet / Nemotron: see each model card (CC-BY-4.0 / NVIDIA Open Model License). Moonshine: MIT. Kroko community models: CC-BY-SA (attribution required). Silero VAD: MIT. Calibration audio: LibriSpeech (CC-BY 4.0).
