<p align="center">
  <img src="data/icons/io.github.captionist.Captionist.svg" width="128" height="128" alt="Captionist logo: a sound wave turning into lines of caption text">
</p>

<h1 align="center">Captionist</h1>

<p align="center">
  <strong>Live captions for everything you hear on Linux. Private, offline and accurate.</strong><br>
  The open-source alternative to Windows Live Captions, for GNOME.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/100%25-offline-2a9d84" alt="100% offline">
  <img src="https://img.shields.io/badge/GNOME-51-4a86cf?logo=gnome&logoColor=white" alt="GNOME 51">
  <img src="https://img.shields.io/badge/made%20with-Rust-b7410e?logo=rust&logoColor=white" alt="Made with Rust">
  <img src="https://img.shields.io/badge/license-GPL--3.0-blue" alt="License: GPL-3.0">
</p>

<p align="center">
  <a href="#features">Features</a> ·
  <a href="#install">Install</a> ·
  <a href="#how-it-works">How it works</a> ·
  <a href="#faq">FAQ</a> ·
  <a href="docs/DEVELOPMENT.md">Develop</a>
</p>

<!-- TODO: add docs/screenshot.png: the caption box over a video, with the ⋮ menu open. -->

---

A Captionist is the person who types live captions as people speak. Inspired by this, **Captionist does that job for you, automatically**: videos, calls, meetings, podcasts, your microphone. Useful in a noisy room, when you can't turn the sound up, for accents and fast talkers, for following a language you're learning, and for anyone who is deaf or hard of hearing. Everything runs on your computer. No account, no cloud, no audio ever leaves your PC.

## Why Captionist

- **🔒 Private by design.** Speech recognition runs entirely on your PC. Captionist works with the network unplugged.
- **🎯 Corrects itself.** You see words instantly; a second, more accurate model then rewrites each sentence when it ends. **About 97% of words correct** on standard English test speech.
- **🎧 Captions anything.** Your speakers, your microphone, both, or **just one app** (for example only your browser, not your music).
- **⚡ Runs on modest hardware.** No GPU needed: tested on a 4-core Intel N150 mini-PC, where it keeps up several times faster than real time.

## Features

| | |
|---|---|
| **Live + corrected captions** | Draft text appears as people speak, then each sentence is replaced by a more accurate version |
| **Floating caption box** | Drag it anywhere, resize from any edge, snap to the top or bottom. Stays visible over fullscreen video |
| **Quick controls** | Hover the box for ⋮ (text colour, background, opacity, size, lines) and × (turn off) |
| **Top bar and Quick Settings** | One click or **Super + Ctrl + L** to turn captions on or off, and switch the audio source |
| **Picks the right model for your PC** | A 10-second check (with a 54 MB test model) estimates every model's speed on your PC; the recommended set downloads with one click, and other models only when you choose them |
| **Custom vocabulary** | Teach it names and jargon ("cooper netties" → "Kubernetes") and set "always write X as Y" rules |
| **History and export** | Scroll back through captions; export as .txt, .srt or .vtt subtitles |
| **Optional acceleration** | Detects NVIDIA and Intel GPUs and NPUs and *measures* whether they're actually faster before using them |
| **Profanity filter** | Optional swear masking |

### Captionist vs Windows Live Captions

| | Windows 11 Live Captions | **Captionist** |
|---|:-:|:-:|
| Works offline, on-device | ✓ | ✓ |
| Speakers and microphone | ✓ | ✓ |
| Docked or floating caption window | ✓ | ✓ (floating, snaps top/bottom) |
| **Caption a single app** | Default output device only | ✓ |
| Open source | — | ✓ |
| Runs on Linux | — | ✓ |

Plus what's listed above: sentence correction, custom vocabulary, history and subtitle export.

## Install

**Requirements:** GNOME 51 on Wayland (Ubuntu 26.10 or newer), PipeWire, a 64-bit x86 PC with 4+ cores and about 2 GB of free RAM.

1. Download `captionist_<version>_amd64.deb` from the [latest release](https://github.com/m-umair-khan-dev/captionist/releases/latest).
2. Install it:
   ```sh
   sudo apt install ./captionist_*_amd64.deb
   ```
3. **Log out and back in once**, so GNOME loads Captionist's top-bar controls.
4. Open **Captionist** from the app grid → *Models* → **Download recommended**.
5. Press **Super + Ctrl + L** and play something.

**Updates are automatic.** The package adds Captionist's signed update source, so new versions arrive with your normal system updates (Software & Updates / `sudo apt upgrade`). After an update, Captionist asks you to log out once to finish.

<details>
<summary>Prefer to add the repository yourself?</summary>

```sh
# Same files the package installs, so the two never conflict.
wget -qO- https://m-umair-khan-dev.github.io/captionist/apt/captionist.asc | sudo tee /usr/share/keyrings/captionist.asc > /dev/null
printf '%s\n' 'Types: deb' 'URIs: https://m-umair-khan-dev.github.io/captionist/apt' 'Suites: stable' 'Components: main' 'Architectures: amd64' 'Signed-By: /usr/share/keyrings/captionist.asc' | sudo tee /etc/apt/sources.list.d/captionist.sources > /dev/null
sudo apt update && sudo apt install captionist
```
</details>

<details>
<summary>Uninstall</summary>

```sh
sudo apt remove captionist
rm -rf ~/.local/share/captionist ~/.local/state/captionist   # downloaded models and history
```
</details>

## How it works

```
your speakers / mic / one app
            │  (PipeWire)
            ▼
   speech detector ── finds where each sentence starts and ends
            │
   ┌────────┴─────────┐
   ▼                  ▼
 live model      correction model
 (Kroko)         (NVIDIA Parakeet)
 words appear    rewrites the finished
 instantly       sentence, ~1 s later
   └────────┬─────────┘
            ▼
   your vocabulary + filters → caption box
```

Measured on an Intel N150 (4 cores), English test speech ([LibriSpeech](https://www.openslr.org/12)):

| Step | Model | Speed | Words correct |
|---|---|---|---|
| Live text | Kroko | ≈ 20× faster than real time | 91% |
| Correction | Parakeet TDT 0.6B v2 | ≈ 6× faster than real time | **97%** |

Real-world audio (music, crosstalk, accents) scores lower than test recordings, as it does for every speech recognizer.

## FAQ

**Does any audio or text leave my computer?**
No. Captions, audio and history stay on your PC. Models are downloaded once from their public releases. Crash reports are **off unless you turn them on**, and never contain captions or audio. Details: [PRIVACY.md](PRIVACY.md).

**Which languages are supported?**
English today. Multilingual models (25 European languages) are next on the [roadmap](#roadmap).

**Do I need a GPU?**
No. Captionist is built to run on the processor. If you have an NVIDIA or Intel GPU, *Settings → Models → Acceleration* measures whether it helps on your PC and only recommends it when it does.

**My PC feels slow while captioning.**
Turn off *Settings → Accuracy → Correct each sentence* to run only the live model, or pick a lighter model in *Settings → Models* (sorted by speed, colour-coded for your PC).

**Does it work on KDE, X11 or other distributions?**
Not yet. GNOME on Wayland only for now; other desktops are on the roadmap. A Flatpak isn't possible for the GNOME part (Flatpak can't install Shell extensions).

## Roadmap

- [ ] More languages (Parakeet v3, Nemotron multilingual) and automatic language detection
- [ ] Offline translation shown under the original captions
- [ ] Speaker labels ("Speaker 1 / Speaker 2") in history and exports
- [ ] KDE Plasma and other desktops
- [ ] Faster correction step after short pauses

## Contributing

Bug reports, translations and pull requests are welcome. Start with **[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)**: everything builds in Docker, no local Rust toolchain needed.

## Credits

Built on [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) and [ONNX Runtime](https://onnxruntime.ai). Speech models: [Kroko](https://huggingface.co/Banafo/Kroko-ASR) by Banafo (CC-BY-SA), [Parakeet and Nemotron](https://huggingface.co/nvidia) by NVIDIA, [Moonshine](https://github.com/moonshine-ai/moonshine) (MIT), [Silero VAD](https://github.com/snakers4/silero-vad) (MIT). Calibration audio from LibriSpeech (CC-BY 4.0).

## License

Captionist is free software under the [GNU GPL v3.0 or later](LICENSE).

<p align="center"><sub>Made for everyone who'd rather read than miss a word.</sub></p>
