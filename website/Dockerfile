# syntax=docker/dockerfile:1

# Build a static musl binary, then ship it alone in an empty image.
FROM rust:1.99.1-alpine AS build
RUN apk add --no-cache musl-dev
# Topcoat downloads a glibc Tailwind CLI, which does not run on Alpine, so the
# musl build is fetched here, pinned by hash. x86_64 only: for arm64 use the
# linux-arm64-musl release and its hash.
ARG TAILWIND_VERSION=4.3.2
ARG TAILWIND_SHA256=ae828e9e989ecbddb2bef856af8b0308ba162583b4922b3a065b5e26f86b0691
RUN wget -q -O /usr/local/bin/tailwindcss "https://github.com/tailwindlabs/tailwindcss/releases/download/v${TAILWIND_VERSION}/tailwindcss-linux-x64-musl" \
    && echo "${TAILWIND_SHA256}  /usr/local/bin/tailwindcss" | sha256sum -c - \
    && chmod +x /usr/local/bin/tailwindcss
ENV TAILWIND_CLI=/usr/local/bin/tailwindcss
WORKDIR /app
# The context is the repository root; the other members are only read by
# Cargo to resolve the shared lock file (see Dockerfile.dockerignore).
COPY Cargo.toml Cargo.lock ./
COPY cli/Cargo.toml cli/Cargo.toml
COPY cli/src cli/src
COPY tools/Cargo.toml tools/Cargo.toml
COPY tools/src tools/src
COPY training/tools/Cargo.toml training/tools/Cargo.toml
COPY training/tools/src training/tools/src
COPY website/Cargo.toml website/build.rs website/styles.css website/
COPY website/src website/src
COPY website/assets website/assets
# The cache mounts keep crates and compiled dependencies between builds;
# the binary is copied out because the target cache is not part of the layer.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --profile website --locked -p smart-commit-website \
    && cp target/website/smart-commit-website /server

FROM scratch
COPY --from=build /server /server
ENV HOST=0.0.0.0 PORT=3000
EXPOSE 3000
USER 10001:10001
HEALTHCHECK --interval=30s --timeout=3s --start-period=2s --retries=3 \
    CMD ["/server", "--health"]
ENTRYPOINT ["/server"]
