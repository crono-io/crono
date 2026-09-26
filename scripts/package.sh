#!/usr/bin/env bash
# Build .deb and .rpm packages for crono-server, crono-worker, and crono-cli
# from prebuilt static Linux binaries with nfpm.
#
# Usage: scripts/package.sh VERSION ARCH BIN_DIR [OUT_DIR]
#   VERSION  release version (X.Y.Z) or a snapshot label such as snapshot-abc1234
#   ARCH     amd64 or arm64; nfpm names them x86_64 and aarch64 in .rpm files
#   BIN_DIR  repository-relative directory holding crono-server,
#            crono-server-openapi, crono-worker, and crono
#   OUT_DIR  repository-relative output directory (default target/packages)
#
# The binaries are the static musl builds from the release, so the packages
# have no libc dependency and install on any systemd-based distribution. nfpm
# runs from a pinned container image unless an `nfpm` binary is on PATH.
set -euo pipefail

readonly nfpm_image="docker.io/goreleaser/nfpm:v2.47.0"

if (($# < 3)); then
  sed -n '2,15p' "$0" >&2
  exit 2
fi
readonly version="$1" arch="$2" bin_dir="$3" out_dir="${4:-target/packages}"

case "$arch" in
  amd64 | arm64) ;;
  *)
    echo "unsupported ARCH ${arch}; expected amd64 or arm64" >&2
    exit 2
    ;;
esac
for path in "$bin_dir" "$out_dir"; do
  if [[ "$path" == /* || "$path" == *..* ]]; then
    echo "${path} must be a path inside the repository" >&2
    exit 2
  fi
done

# Package versions must start with a digit. A snapshot label becomes a `~`
# pre-release of 0.0.0, which sorts before every real release.
if [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  package_version="$version"
else
  package_version="0.0.0~${version//-/.}"
fi

cd "$(git rev-parse --show-toplevel)"
for binary in crono-server crono-server-openapi crono-worker crono; do
  if [[ ! -x "${bin_dir}/${binary}" ]]; then
    echo "missing executable ${bin_dir}/${binary}" >&2
    exit 1
  fi
done
mkdir -p "$out_dir"

# nfpm does not expand variables in file sources, so the definitions read the
# binaries from one fixed staging directory.
readonly staging="target/package-bin"
rm -rf "$staging"
mkdir -p "$staging"
cp "${bin_dir}/crono-server" "${bin_dir}/crono-server-openapi" \
  "${bin_dir}/crono-worker" "${bin_dir}/crono" "$staging/"

if command -v nfpm >/dev/null 2>&1; then
  nfpm=(nfpm)
else
  engine="$(command -v podman || command -v docker || true)"
  if [[ -z "$engine" ]]; then
    echo "nfpm, podman, or docker is required" >&2
    exit 1
  fi
  nfpm=("$engine" run --rm --volume "$PWD:/work:z" --workdir /work
    --env VERSION --env ARCH "$nfpm_image")
fi

export VERSION="$package_version" ARCH="$arch"
for package in crono-server crono-worker crono-cli; do
  for packager in deb rpm; do
    "${nfpm[@]}" package --config "packaging/nfpm/${package}.yaml" \
      --packager "$packager" --target "${out_dir}/"
  done
done
