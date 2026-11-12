# Build image for Captionist: Ubuntu 24.04 LTS, the oldest supported release,
# so one .deb runs on 24.04 and everything newer (programs built against older
# glibc/GTK run on newer ones, not the other way round), + the Rust toolchain
# copied from the devcontainer image.
FROM mcr.microsoft.com/devcontainers/rust:1-bookworm AS rust
FROM ubuntu:24.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential pkg-config ca-certificates curl \
      libgtk-4-dev libadwaita-1-dev libglib2.0-dev-bin dpkg-dev cmake git \
 && rm -rf /var/lib/apt/lists/*
COPY --from=rust /usr/local/rustup /usr/local/rustup
COPY --from=rust /usr/local/cargo /usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup PATH=/usr/local/cargo/bin:$PATH
RUN chmod -R a+rX /usr/local/rustup /usr/local/cargo && rustc --version
