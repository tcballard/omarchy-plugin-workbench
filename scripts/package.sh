#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
version="$(jq -r '.version' "$repo_root/manifest.json")"
archive_name="omarchy-discovery-${version}-x86_64.tar.gz"
archive_root="omarchy-discovery-${version}"

cargo build --manifest-path "$repo_root/Cargo.toml" --workspace --locked --release
install -m 0755 \
  "$repo_root/target/release/omarchy-discovery" \
  "$repo_root/bin/omarchy-discovery-x86_64"

"$repo_root/scripts/validate.sh"
mkdir -p "$repo_root/dist"
temporary_archive="$repo_root/dist/.${archive_name}.tmp"
tar \
  --format=gnu \
  --sort=name \
  --mtime='@0' \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  --exclude='./.git' \
  --exclude='./target' \
  --exclude='./dist' \
  --transform="s,^\./,$archive_root/," \
  -C "$repo_root" \
  -cf - \
  . | gzip -n > "$temporary_archive"
mv "$temporary_archive" "$repo_root/dist/$archive_name"
chmod 0644 "$repo_root/dist/$archive_name"

(
  cd "$repo_root/dist"
  sha256sum "$archive_name" > SHA256SUMS
)

printf '%s\n' "$repo_root/dist/$archive_name"
