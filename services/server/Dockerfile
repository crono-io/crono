# syntax=docker/dockerfile:1
#
# crono-server image: one static musl executable on distroless/static.
#
# Stages:
# - `base`: the runtime base pinned by digest, with labels, user and entrypoint.
# - `chef`, `planner`, `builder`: compile the binary from the repository root.
#   cargo-chef reduces the workspace to its manifests (`recipe.json`) and
#   compiles the dependencies in their own layer, which only a change to a
#   manifest or `Cargo.lock` rebuilds; BuildKit exports that layer to the CI
#   cache, which cache mounts would not be.
# - `prebuilt`: copies an executable staged as `<platform>/crono-server` in the
#   build context (for example `linux/arm64/crono-server`). The release builds
#   this target from the binaries it publishes in the release archives, so the
#   image ships exactly those bytes and multi-platform builds need no emulation;
#   `just images` stages the same layout locally.
# - `runtime` (last, the default): copies the binary from `builder`, so
#   `docker build -f services/server/Dockerfile .` works from a plain checkout.
#
# The build context keeps `.git`: `built` records the commit in `--version`.
# Pass `--build-arg SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)` for a
# reproducible image creation time; the binary embeds no build time because
# `built` runs without its `chrono` feature.
#
# distroless/static contains CA certificates, tzdata, and a `nonroot` user
# (65532) but no shell or package manager. To update a base, resolve the index
# digest of its tag and replace both, for example:
#   docker buildx imagetools inspect gcr.io/distroless/static-debian13:nonroot
#   docker buildx imagetools inspect docker.io/library/rust:1.99.1-alpine

FROM gcr.io/distroless/static-debian13:nonroot@sha256:e2e927ec666bae08560abb3c55d0659eceabb657f56b6782ab500a9fc7f555e3 AS base

ARG VERSION=dev
ARG REVISION=unknown
ARG CREATED=unknown
LABEL org.opencontainers.image.title="crono-server" \
      org.opencontainers.image.description="Crono control-plane API, scheduler, and outbox publisher" \
      org.opencontainers.image.source="https://github.com/crono-io/crono" \
      org.opencontainers.image.licenses="BSD-3-Clause" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.created="${CREATED}"

USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/crono-server"]
# Info-level structured logs; pass other arguments to change verbosity.
CMD ["-v"]

# rust:alpine targets musl, whose binaries are static by default; `ring`
# compiles C, hence musl-dev.
FROM docker.io/library/rust:1.99.1-alpine@sha256:7e9e2d068a107a2adafd19930011178d14a78326053cd4c3a9f7ad3fd049557d AS chef
RUN apk add --no-cache musl-dev \
    && cargo install cargo-chef --version 0.1.78 --locked \
    && rm -rf "${CARGO_HOME}/registry"
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --locked -p crono-server --recipe-path recipe.json
COPY . .
# Declared after the dependency layer so a new timestamp never invalidates it.
ARG SOURCE_DATE_EPOCH
RUN cargo build --release --locked -p crono-server --bin crono-server

FROM base AS prebuilt
ARG TARGETPLATFORM
COPY --chmod=0555 ${TARGETPLATFORM}/crono-server /usr/local/bin/crono-server

FROM base AS runtime
COPY --from=builder --chmod=0555 /src/target/release/crono-server /usr/local/bin/crono-server
