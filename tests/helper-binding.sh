#!/bin/bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
test_dir="$(mktemp -d)"
trap 'rm -rf -- "$test_dir"' EXIT

helper="$test_dir/omarchy-plugin-workbench"
wrapper="$test_dir/wrapper"
cp -- /usr/bin/printf "$helper"
digest="$(/usr/bin/sha256sum -- "$helper")"
digest="${digest%% *}"

# Rewrite only the pinned location and hash in a throwaway copy. The production
# wrapper has no environment override that an installed plugin could exploit.
sed \
  -e "s|^helper=/usr/bin/omarchy-plugin-workbench$|helper=$helper|" \
  -e "s|^expected_sha256=.*$|expected_sha256=$digest|" \
  "$repo_root/bin/omarchy-plugin-workbench" > "$wrapper"
chmod 0700 "$wrapper"

[[ "$("$wrapper" '%s' 'approved')" == approved ]]

cp -- /usr/bin/true "$helper"
if "$wrapper" '%s' 'wrong binary' > "$test_dir/output" 2> "$test_dir/error"; then
  echo 'A different executable was accepted' >&2
  exit 1
fi
grep -Fq 'helper digest mismatch' "$test_dir/error"
[[ ! -s $test_dir/output ]]

rm -- "$helper"
ln -s /usr/bin/printf "$helper"
if "$wrapper" '%s' 'linked binary' > "$test_dir/output" 2> "$test_dir/error"; then
  echo 'A linked executable was accepted' >&2
  exit 1
fi
grep -Fq 'requires the reviewed' "$test_dir/error"
[[ ! -s $test_dir/output ]]

rm -- "$helper"
if "$wrapper" '%s' 'missing binary' > "$test_dir/output" 2> "$test_dir/error"; then
  echo 'A missing executable was accepted' >&2
  exit 1
fi
grep -Fq 'requires the reviewed' "$test_dir/error"
[[ ! -s $test_dir/output ]]

echo 'Helper identity checks passed'
