# Changelog

All notable changes to Captionist are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/). From 0.1.0 on, entries are
generated from commit messages by release-please (see [CONTRIBUTING.md](CONTRIBUTING.md)).

## [0.1.1](https://github.com/m-umair-khan-dev/captionist/compare/v0.1.0...v0.1.1) (2026-11-11)


### Bug Fixes

* update zip and other dependencies to resolve security advisories ([c6c5947](https://github.com/m-umair-khan-dev/captionist/commit/c6c59472bed1f2de548bdaaa443f3f1270f2b376))

## 0.1.0 (unreleased)

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
