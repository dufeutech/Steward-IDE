# The Unix host this project does not otherwise have.
#
# `terminal_pty.rs` is written for both platforms — its Unix arms use `/bin/sh`, `stty
# size` and `ping -c 25` — but until this image existed there was nowhere to run them, and
# `terminal-surface` task 7.4 carried "Unix remains unverified" forward across sessions.
#
# The system packages are the same four `.github/workflows/checks.yml` installs, so a pass
# here means the same thing a pass on CI's runner would. `iputils-ping` is the addition:
# the interrupt tests discriminate on a ~21 s command against a 2 s budget, and without a
# real `ping` they would pass without measuring anything.
FROM rust:1-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
      libwebkit2gtk-4.1-dev \
      libappindicator3-dev \
      librsvg2-dev \
      patchelf \
      iputils-ping \
      procps \
 && rm -rf /var/lib/apt/lists/*

# Kept off the mounted source tree: the repository is checked out on Windows and already
# holds a `target/` full of Windows artifacts. Sharing it would have the two toolchains
# overwrite each other's fingerprints.
ENV CARGO_TARGET_DIR=/target

WORKDIR /src/app/src-tauri
