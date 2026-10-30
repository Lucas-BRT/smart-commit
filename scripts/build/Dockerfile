# The release build environment, pinned so the same commit builds the same
# binary on any machine: the image by digest, the system packages (gcc,
# libc6-dev, whose C runtime objects end up in the binary) by an Ubuntu
# snapshot date, rustup by SHA-256, and the Rust toolchain by
# rust-toolchain.toml. Ubuntu 22.04's glibc makes the binary run on 22.04
# and newer. The image is built from this file once, published to the
# registry by the Build image workflow and pulled by digest from
# scripts/build/IMAGE; this file is its recipe.
FROM ubuntu:22.04@sha256:0a9887931b0e38af2e3487ed39e29436b0535c4f3f52e57745baaf79d1898956

ARG SNAPSHOT=20261001T000000Z
# The snapshot service needs HTTPS, so CA certificates come first, from the
# image's own archive; they never reach the binary, so their version does
# not matter. Everything that does comes from the snapshot.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && for suite in jammy jammy-updates jammy-security; do \
        echo "deb https://snapshot.ubuntu.com/ubuntu/$SNAPSHOT $suite main universe"; \
    done >/etc/apt/sources.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends curl gcc libc6-dev \
    && rm -rf /var/lib/apt/lists/*

ARG RUSTUP_VERSION=1.29.1
ARG RUSTUP_SHA256=dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71
RUN curl -fsSL -o /tmp/rustup-init \
        "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/x86_64-unknown-linux-gnu/rustup-init" \
    && echo "$RUSTUP_SHA256  /tmp/rustup-init" | sha256sum --check --quiet - \
    && chmod +x /tmp/rustup-init \
    && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain none \
    && rm /tmp/rustup-init
ENV PATH=/root/.cargo/bin:$PATH
