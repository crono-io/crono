# syntax=docker/dockerfile:1
#
# crono-web runtime image: the compiled single-page app behind unprivileged
# nginx on port 8080.
#
# The bundle is not built here. The release workflow runs `trunk build
# --release` once and stages the result as `web/dist` next to `web/nginx` in
# the build context; `just images` stages the same layout locally. The image
# serves static files only: route `/api` to crono-server at the Ingress or
# Gateway so the browser keeps one origin for the client's relative API calls.
#
# The `alpine-slim` variant is nginx's stable release without optional dynamic
# modules, which keeps the image small; gzip and headers are built in. To
# update the base, resolve the index digest of the new stable tag and replace
# both, for example:
#   docker buildx imagetools inspect nginxinc/nginx-unprivileged:1.30-alpine-slim
FROM docker.io/nginxinc/nginx-unprivileged:1.30.5-alpine-slim@sha256:307ca0bded4dda86dade6f5bd2a2551601bec9bac3fd73a21613fabcd9eca66a

COPY --chmod=0444 web/nginx/default.conf /etc/nginx/conf.d/default.conf
COPY --chmod=0444 web/nginx/security-headers.conf /etc/nginx/snippets/security-headers.conf
COPY web/dist/ /usr/share/nginx/html/

EXPOSE 8080
