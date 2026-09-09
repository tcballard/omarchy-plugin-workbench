#!/usr/bin/env bash
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
mkdir -p "$fixture/bin" "$fixture/target/release"
cp "$repo/bin/omarchy-plugin-workbench" "$fixture/bin/"
cat > "$fixture/target/release/omarchy-plugin-workbench" <<'HELPER'
#!/bin/bash
if [[ $1 == protocol ]]; then echo workbench-protocol-1; else printf '<%s>\n' "$@"; fi
HELPER
chmod +x "$fixture/target/release/omarchy-plugin-workbench"
[[ $("$fixture/bin/omarchy-plugin-workbench" drawer-profile 'space with spaces') == $'<drawer-profile>\n<space with spaces>' ]]
printf '#!/bin/bash\necho incompatible\n' > "$fixture/target/release/omarchy-plugin-workbench"
if [[ ! -x /usr/bin/omarchy-plugin-workbench ]]; then
  if "$fixture/bin/omarchy-plugin-workbench" status 2>/dev/null; then exit 1; fi
fi
echo 'ok - launcher checks protocol and preserves argument boundaries'
