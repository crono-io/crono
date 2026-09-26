# syntax=docker/dockerfile:1
#
# crono-server runtime image.
#
# Nothing is compiled here. The release workflow builds one static musl
# executable per architecture and stages it as `<platform>/crono-server` in the
# build context (for example `linux/arm64/crono-server`), so the image ships
# exactly the binary published in the release archives and multi-platform
# builds need no emulation. `just images` stages the same layout locally.
#
# distroless/static contains CA certificates, tzdata, and a `nonroot` user
# (65532) but no shell or package manager. To update the base, resolve the
# index digest of the tag and replace both, for example:
#   docker buildx imagetools inspect gcr.io/distroless/static-debian13:nonroot
FROM gcr.io/distroless/static-debian13:nonroot@sha256:e2e927ec666bae08560abb3c55d0659eceabb657f56b6782ab500a9fc7f555e3

ARG TARGETPLATFORM
COPY --chmod=0555 ${TARGETPLATFORM}/crono-server /usr/local/bin/crono-server

USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/crono-server"]
# Info-level structured logs; pass other arguments to change verbosity.
CMD ["-v"]
