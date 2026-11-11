# Privacy

Captionist turns speech into text **entirely on your computer**. Your audio,
captions, caption history and custom vocabulary are never uploaded anywhere.

## What Captionist downloads

- **Speech models**, when you choose them in Settings → Models, from their
  official release pages (GitHub). A 54 MB test model is downloaded on first
  start to check your PC's speed.
- **Acceleration packs**, only if you choose *Set up* under Acceleration, from
  PyPI (Intel) or GitHub (Microsoft), checked against pinned SHA-256 checksums.
- **Updates**, through your system's normal package updates.

These are ordinary downloads: like any web request, the server sees your IP address.

## Crash reports (off unless you turn them on)

Captionist asks once whether to send crash reports. You can change your answer
any time in **Settings → General → Privacy and feedback**. If you turn them on,
a report is sent to [Sentry](https://sentry.io) when Captionist crashes or hits
an error (a model fails to load, audio capture stops, a download fails…).

**A report contains:** the error message and stack trace, the Captionist
version, your distribution and GNOME version, the number of CPU cores, the
amount of RAM, and which accelerator is in use (if any).

**A report never contains:** captions, audio, caption history, custom
vocabulary, file names, your user name, your computer's name, or your IP address
(Captionist removes user names, home-folder paths and the computer name before
sending; IP storage is switched off in the Sentry project).

Development and pull-request builds contain no report address, so they can't
send anything.

## Report a problem

**Settings → General → Report a problem** shows a diagnostics summary
(versions, hardware, models in use, recent errors, the last log lines, with
user and computer names removed). Nothing is sent by Captionist: you can copy
it, or open a pre-filled GitHub issue and decide what to post.

## Questions

Open a [discussion](https://github.com/m-umair-khan-dev/captionist/discussions)
or see [SECURITY.md](SECURITY.md) for private reports.
