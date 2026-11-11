# Security policy

## Supported versions

Only the latest release receives fixes. Updates are delivered through the
Captionist APT repository, so installing the latest version is usually just a
normal system update.

## Reporting a vulnerability

Please **don't open a public issue**. Report it privately through GitHub:
**[Report a vulnerability](https://github.com/m-umair-khan-dev/captionist/security/advisories/new)**.

Include what an attacker could do, how to reproduce it, and the Captionist
version. You'll get a reply within 7 days. Once a fix is released, we credit
you in the advisory unless you prefer otherwise.

## What's in scope

- The caption service (`captionistd`), the settings app and the GNOME Shell extension
- The `.deb` package and the APT repository (signing, update path)
- Downloads: acceleration packs are SHA-256-verified against pinned checksums;
  speech models are downloaded over HTTPS from their official release pages

Captionist processes audio only on your computer and has no server of its own.
Problems in third-party speech models or ONNX Runtime should also be reported
to those projects; we'll help coordinate.
