#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 OUTPUT_DIRECTORY" >&2
  exit 64
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
output_dir="$1"
build_image="${WORKBENCH_BUILD_IMAGE:-rust:1.98.0-bookworm}"
cargo_cache="${WORKBENCH_CARGO_CACHE:-${TMPDIR:-/tmp}/omarchy-plugin-workbench-cargo}"

case "$output_dir" in
  /*) ;;
  *) echo "output directory must be absolute" >&2; exit 64 ;;
esac

mkdir -p "$output_dir" "$cargo_cache"

docker run --rm \
  --volume "$repo_root:/src:ro" \
  --volume "$output_dir:/target" \
  --volume "$cargo_cache:/cargo" \
  --workdir /src \
  --env CARGO_HOME=/cargo \
  --env CARGO_INCREMENTAL=0 \
  --env CARGO_TARGET_DIR=/target \
  --env LANG=C.UTF-8 \
  --env LC_ALL=C.UTF-8 \
  --env SOURCE_DATE_EPOCH=1 \
  --env TZ=UTC \
  --env 'RUSTFLAGS=--remap-path-prefix=/src=. -C link-arg=-Wl,--build-id=none' \
  "$build_image" \
  bash -ceu 'cargo fetch --locked; cargo build --workspace --release --frozen'

test -x "$output_dir/release/omarchy-plugin-workbench"
