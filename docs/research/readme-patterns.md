# README research: what popular open-source projects do

Method (2026-11-11): the READMEs of 36 popular open-source projects (general apps,
Linux desktop apps, speech/caption tools; 1.1k–187k GitHub stars) were downloaded
and analysed with `tools/analyze.py`:

- **Feature extraction:** logo, badges, screenshots, headings, tagline length,
  wording rates (privacy / speed / ease / free), emoji, "you" density.
- **Spearman rank correlation** of each feature with log(stars).
- **TF-IDF** of the opening pitch (first ~250 words) against the whole corpus.
- **k-means (k = 3)** on z-scored features to find README archetypes.

36 projects is a small sample: correlations are weak (|r| ≤ 0.2) and show
tendencies, not causes.

## Findings used in Captionist's README

1. **The most-starred archetype** (median ~59k stars: Excalidraw, Immich, AppFlowy,
   Jellyfin…): centred logo header, a few badges, a Features section, *shorter*
   README with fewer sections.
2. **Taglines are short** (median 9 words): category + one differentiator, often
   "the open-source alternative to X".
3. **Privacy wording** (offline, local, private, your data) had the strongest
   positive link to stars (+0.19); **"easy/simple" wording** the strongest negative
   (−0.20). Show ease with steps and numbers instead of claiming it.
4. **Long READMEs and many headings** link slightly to fewer stars (−0.11, −0.16):
   move build details to a separate developer guide.
5. **Closest successful competitor:** Handy (38k stars): "A free, open source, and
   extensible speech-to-text application that works completely offline."

---

## Raw analysis output

## Sample: 36 READMEs, stars 1,108 – 186,660

| repo | stars | cat | tagline |
|---|---|---|---|
| ollama/ollama | 186,660 | general | Start building with open models. Access the latest releases locally or in the cloud with complete privacy. |
| excalidraw/excalidraw | 135,502 | general | An open source virtual hand-drawn style whiteboard. |
| rustdesk/rustdesk | 129,032 | general | [ Українська ] | [ česky ] | [ 中文 ] | [ Magyar ] | [ Español ] | [ فارسی ] | [ Français ] | [ Deutsch ] | [ Po |
| immich-app/immich | 118,846 | general | High performance self-hosted photo and video management solution |
| openai/whisper | 112,454 | speech | Whisper is a general-purpose speech recognition model. It is trained on a large dataset of diverse audio and i |
| localsend/localsend | 98,441 | general | [Homepage][homepage] • [Discord][discord] • [GitHub][github] • [Codeberg][codeberg] |
| Stirling-Tools/Stirling-PDF | 96,290 | general | Stirling PDF - The Open-Source PDF Platform |
| zed-industries/zed | 94,670 | general | Welcome to Zed, a high-performance, multiplayer code editor from the creators of Atom and Tree-sitter. |
| syncthing/syncthing | 90,359 | general | Syncthing is a **continuous file synchronization program**. It synchronizes |
| hoppscotch/hoppscotch | 80,782 | general | Open Source API Development Ecosystem |
| obsproject/obs-studio | 79,361 | general | :alt: OBS Studio Build Status - GitHub Actions |
| AppFlowy-IO/AppFlowy | 78,717 | general | ⭐️  The Open Source Alternative To Notion  ⭐️ |
| BurntSushi/ripgrep | 69,891 | general | ripgrep is a line-oriented search tool that recursively searches the current |
| alacritty/alacritty | 66,167 | general | Alacritty - A fast, cross-platform, OpenGL terminal emulator |
| sharkdp/bat | 61,215 | general | A  cat(1)  clone with syntax highlighting and Git integration. |
| jellyfin/jellyfin | 59,215 | general | The Free Software Media System |
| ggml-org/whisper.cpp | 55,098 | speech | High-performance inference of OpenAI's Whisper automatic speech recognition (ASR) model: |
| upscayl/upscayl | 52,262 | general | v2.15 is out! 🥳 Download Now ⬇️ |
| logseq/logseq | 45,558 | general | A privacy-first, open-source platform for knowledge management and collaboration |
| cjpais/Handy | 38,407 | speech | A free, open source, and extensible speech-to-text application that works completely offline. |
| mpv-player/mpv | 38,067 | general | mpv** is a free (as in freedom) media player for the command line. It supports |
| flameshot-org/flameshot | 31,512 | linux | Powerful yet simple to use screenshot software. |
| SYSTRAN/faster-whisper | 26,668 | speech | Faster Whisper transcription with CTranslate2 |
| m-bain/whisperX | 25,214 | speech | Recall.ai - Meeting Transcription API |
| chidiwilliams/buzz | 22,635 | speech | Transcribe and translate audio offline on your personal computer. Powered by |
| k2-fsa/sherpa-onnx | 15,883 | speech | Speech recognition| [Speech synthesis][tts-url] | [Source separation][ss-url] |
| rhasspy/piper | 11,323 | speech | Development has moved: https://github.com/OHF-Voice/piper1-gpl |
| HeroTools/open-whispr | 11,288 | speech | The open-source and free alternative to WisprFlow and Granola. |
| bottlesdevs/Bottles | 9,067 | linux | Run Windows Software on Linux |
| Vencord/Vesktop | 8,609 | linux | Vesktop is a custom Discord desktop app |
| pop-os/cosmic-epoch | 7,003 | linux | COSMIC is a desktop environment offering performance, efficiency, and personalization to empower a wide variet |
| mijorus/gearlever | 2,391 | linux | Integrate AppImages into your app menu with **just one click |
| abb128/LiveCaptions | 1,807 | speech | Live Captions is an application that provides live captioning for the Linux desktop. |
| mkiol/dsnote | 1,713 | speech | Linux desktop and Sailfish OS app for note taking, reading and translating with offline Speech to Text, Text t |
| nokyan/resources | 1,276 | linux | ⚠️ Resources is now part of GNOME Incubator! All further development and discussion will happen on GNOME GitLa |
| savbell/whisper-writer | 1,108 | speech | Update (2024-05-28):** I've just merged in a major rewrite of WhisperWriter! We've migrated from using   to us |

## Feature prevalence (share of READMEs / median)

- words: median 742.0
- logo_top: 44% of READMEs
- centered_header: 58% of READMEs
- badges: median 2.0
- screenshots: median 2.0
- gif_or_video: 31% of READMEs
- headings: median 13.0
- install_pos: median 0.2
- has_features_section: 31% of READMEs
- has_why_section: 11% of READMEs
- has_faq: 8% of READMEs
- has_comparison_table: 3% of READMEs
- bullets: median 20.0
- emoji_per_1k: median 0.0
- you_per_1k: median 18.9
- privacy_per_1k: median 2.6
- speed_per_1k: median 1.7
- ease_per_1k: median 1.3
- free_per_1k: median 1.7
- tagline_words: median 9.0

## Spearman correlation with log(stars)  (n is small: indicative only)

- privacy_per_1k: +0.19
- install_pos: +0.18
- free_per_1k: +0.17
- badges: +0.15
- screenshots: +0.14
- centered_header: +0.11
- has_why_section: +0.09
- emoji_per_1k: +0.04
- logo_top: +0.03
- speed_per_1k: +0.03
- has_comparison_table: +0.02
- has_faq: +0.01
- has_features_section: +0.00
- tagline_words: -0.07
- words: -0.11
- bullets: -0.11
- you_per_1k: -0.11
- gif_or_video: -0.15
- headings: -0.16
- ease_per_1k: -0.20

## Most common section headings

- contributing: 13
- license: 12
- installation: 11
- features: 8
- building: 7
- linux: 6
- macos: 5
- windows: 5
- quick start: 5
- sponsors: 5
- documentation: 4
- download: 4
- usage: 4
- changelog: 4
- prerequisites: 4
- docker: 3
- screenshots: 3
- build: 3
- contributors: 3
- translations: 3
- getting started: 3
- troubleshooting: 3
- acknowledgments: 3
- roadmap: 3
- faq: 3

## TF-IDF: most distinctive words in opening pitches

moved, ohf-voice, yes, gpl, piper, localsend, zed, obsproject, whisper, buzz, cosmic, vesktop, alacritty, pop, excalidraw, cuda, ripgrep, logseq, tts, speech, gpu, syncthing, bottles, development, discord, handy, obs-studio, resources, arm, desktop, flatpak, recording, upscayl, shortcut, model, faster-whisper, nvidia, speaker, app, grep, syntax, building, build, ollama, jellyfin

## Pitch words used more by the top-starred half

documentation (+8), translations (+7), details (+7), channels (+7), support (+6), started (+6), quick (+6), other (+6), here (+6), contributing (+6), rest (+5), many (+5), guide (+5), community (+5), without (+4), where (+4), start (+4), solution (+4), server (+4), see (+4), read (+4), public (+4), project (+4), open-source (+4), like (+4)

## k-means archetypes (k=3, z-scored features)

### Cluster 0: 13 repos, median stars 38,407
high: install_pos (+0.3σ), free_per_1k (+0.2σ), ease_per_1k (+0.2σ), has_why_section (+0.1σ), you_per_1k (+0.1σ)
low: gif_or_video (-0.7σ), badges (-0.7σ), centered_header (-1.0σ)
repos: openai/whisper, localsend/localsend, zed-industries/zed, syncthing/syncthing, obsproject/obs-studio, BurntSushi/ripgrep, cjpais/Handy, mpv-player/mpv, rhasspy/piper, Vencord/Vesktop, pop-os/cosmic-epoch, abb128/LiveCaptions, nokyan/resources

### Cluster 1: 7 repos, median stars 31,512
high: headings (+1.7σ), words (+1.5σ), bullets (+1.3σ), gif_or_video (+0.9σ), has_comparison_table (+0.7σ)
low: has_why_section (-0.4σ), you_per_1k (-0.4σ), logo_top (-0.6σ)
repos: ollama/ollama, sharkdp/bat, ggml-org/whisper.cpp, flameshot-org/flameshot, SYSTRAN/faster-whisper, k2-fsa/sherpa-onnx, mkiol/dsnote

### Cluster 2: 16 repos, median stars 59,215
high: logo_top (+0.7σ), centered_header (+0.7σ), badges (+0.5σ), emoji_per_1k (+0.5σ), has_features_section (+0.4σ)
low: bullets (-0.3σ), headings (-0.4σ), words (-0.4σ)
repos: excalidraw/excalidraw, rustdesk/rustdesk, immich-app/immich, Stirling-Tools/Stirling-PDF, hoppscotch/hoppscotch, AppFlowy-IO/AppFlowy, alacritty/alacritty, jellyfin/jellyfin, upscayl/upscayl, logseq/logseq, m-bain/whisperX, chidiwilliams/buzz, HeroTools/open-whispr, bottlesdevs/Bottles, mijorus/gearlever, savbell/whisper-writer
