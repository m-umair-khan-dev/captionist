# Changelog

All notable changes to Captionist are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/). From 0.1.0 on, entries are
generated from commit messages by release-please (see [CONTRIBUTING.md](CONTRIBUTING.md)).

## [0.1.1](https://github.com/m-umair-khan-dev/captionist/compare/v0.1.0...v0.1.1) (2026-11-11)


### Bug Fixes

* serve the caption service over GIO D-Bus instead of zbus ([#9](https://github.com/m-umair-khan-dev/captionist/issues/9)) ([c846523](https://github.com/m-umair-khan-dev/captionist/commit/c84652399edf6df101c5f9d337a2913675595d78))

## 0.1.0 (2026-11-11)

First public preview.

### Features

- Live captions for speakers, microphone, both, or a single app, fully offline.
- Two-pass recognition: instant draft text (Kroko), each sentence then corrected (NVIDIA Parakeet TDT 0.6B v2).
- Hardware check that estimates every model's speed on the PC and recommends the best set; models download on request.
- Floating caption box: drag to move, resize from any edge, snap to top or bottom, ⋮ quick menu (colours, opacity, size, lines), × to turn off.
- Top-bar menu, Quick Settings toggle and Super + Ctrl + L shortcut.
- Settings window: audio source, models (sortable, colour-coded fit), appearance, accuracy, history.
- Custom vocabulary with sound-alike matching, and "always write X as Y" replacements.
- Caption history with .txt, .srt and .vtt export.
- Optional hardware acceleration (NVIDIA CUDA, Intel OpenVINO GPU/NPU) that is measured before it's recommended.
- Profanity filter.
- Opt-in crash reports (off unless turned on) and "Report a problem" with a reviewable diagnostics summary.
- Signed APT repository: installing the .deb once enables automatic updates.

### Security

- Updated `zip` to 9.x, resolving four advisories in the archive reader used for acceleration packs.
