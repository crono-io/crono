# syntax=docker/dockerfile:1
#
# crono-web image: the compiled single-page app behind unprivileged nginx on
# port 8080. It serves static files only: route `/api` to crono-server at the
# Ingress or Gateway so the browser keeps one origin for the client's relative
# API calls.
#
# Stages:
# - `base`: the runtime base pinned by digest, with labels and user.
# - `chef`, `planner`, `builder`: build the bundle with `trunk build
#   --release` from the repository root. cargo-chef compiles the wasm32
#   dependencies in their own layer, which only a change to a manifest or
#   `Cargo.lock` rebuilds; BuildKit exports that layer to the CI cache.
# - `prebuilt`: copies a bundle staged as `web/dist` next to `web/nginx` in the
#   build context. The release builds this target from the bundle it publishes,
#   so the image serves exactly those files; `just images` stages the same
#   layout locally.
# - `runtime` (last, the default): copies the bundle from `builder`, so
#   `docker build -f apps/web/Dockerfile .` works from a plain checkout.
#
# Trunk downloads wasm-bindgen, wasm-opt and tailwindcss at build time; they
# are glibc binaries, hence the Debian-based builder.
#
# The `alpine-slim` variant is nginx's stable release without optional dynamic
# modules, which keeps the image small; gzip and headers are built in. To
# update a base, resolve the index digest of its tag and replace both, for
# example:
#   docker buildx imagetools inspect nginxinc/nginx-unprivileged:1.30-alpine-slim
#   docker buildx imagetools inspect docker.io/library/rust:1.99.1-slim-trixie

FROM docker.io/nginxinc/nginx-unprivileged:1.30.6-alpine-slim@sha256:5d8bb9437d74a600ec5dbf90c035451c724dc33eac8d3a71078f6f8612c345f6 AS base

ARG VERSION=dev
ARG REVISION=unknown
ARG CREATED=unknown
LABEL org.opencontainers.image.title="crono-web" \
      org.opencontainers.image.description="Crono web client served by unprivileged nginx" \
      org.opencontainers.image.source="https://github.com/crono-io/crono" \
      org.opencontainers.image.licenses="BSD-3-Clause" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.created="${CREATED}"

# The base image's nginx user and group; its entrypoint and command stay.
USER 101:101
EXPOSE 8080

# Trunk's version matches the workflows' `cargo install trunk`.
FROM docker.io/library/rust:1.99.1-slim-trixie@sha256:b8e062b1c3a80183df83d9ba187cadd2b5ab84bc78d453dbfeb2fccd5f982247 AS chef
ENV TRUNK_SKIP_VERSION_CHECK=true
RUN rustup target add wasm32-unknown-unknown \
    && cargo install cargo-chef --version 0.1.78 --locked \
    && cargo install trunk --version 0.21.14 --locked \
    && rm -rf "${CARGO_HOME}/registry"
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --locked --target wasm32-unknown-unknown -p crono-web --recipe-path recipe.json
COPY . .
WORKDIR /src/apps/web
RUN trunk build --release

FROM base AS prebuilt
COPY --chmod=0444 web/nginx/default.conf /etc/nginx/conf.d/default.conf
COPY --chmod=0444 web/nginx/security-headers.conf /etc/nginx/snippets/security-headers.conf
COPY web/dist/ /usr/share/nginx/html/

FROM base AS runtime
COPY --chmod=0444 apps/web/nginx/default.conf /etc/nginx/conf.d/default.conf
COPY --chmod=0444 apps/web/nginx/security-headers.conf /etc/nginx/snippets/security-headers.conf
COPY --from=builder /src/apps/web/dist/ /usr/share/nginx/html/
