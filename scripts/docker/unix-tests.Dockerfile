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
#
# It also produces this project's Linux bundles (`binary-release-pipeline` task 2.3). That is
# the same job the release runner does, so proving it here first means the workflow's Linux
# entry is not the place we discover a missing system package.
FROM rust:1-bookworm

# The first six are the test dependencies. `file`, `xdg-utils`, `libxdo-dev` and `libssl-dev`
# are what bundling adds on top, from Tauri's own Linux prerequisites list; `wget` fetches the
# AppImage tooling, which the bundler downloads at bundle time rather than vendoring.
RUN apt-get update && apt-get install -y --no-install-recommends \
      libwebkit2gtk-4.1-dev \
      libappindicator3-dev \
      librsvg2-dev \
      patchelf \
      iputils-ping \
      procps \
      file \
      wget \
      xdg-utils \
      libxdo-dev \
      libssl-dev \
 && rm -rf /var/lib/apt/lists/*

# The CLI comes from crates.io rather than npm because this image has no Node and needs none:
# `frontendDist` is a static directory, so there is no frontend build step to run. Last in the
# file because it is the slow layer — everything above stays cached when it changes.
RUN cargo install tauri-cli --locked --version "^2"

# Kept off the mounted source tree: the repository is checked out on Windows and already
# holds a `target/` full of Windows artifacts. Sharing it would have the two toolchains
# overwrite each other's fingerprints.
ENV CARGO_TARGET_DIR=/target

WORKDIR /src/app/src-tauri
