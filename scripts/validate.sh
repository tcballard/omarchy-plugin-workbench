#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"

jq -e '
  .schemaVersion == 1
  and .id == "io.github.tcballard.plugin-workbench"
  and (.kinds | index("bar-widget") != null)
  and .entryPoints.barWidget == "BarWidget.qml"
' "$repo_root/manifest.json" >/dev/null

[[ -f $repo_root/BarWidget.qml ]]
[[ -f $repo_root/Panel.qml ]]
[[ -x $repo_root/bin/omarchy-plugin-workbench ]]

if command -v omarchy >/dev/null 2>&1; then
  omarchy plugin validate "$repo_root"
fi
if command -v qmllint >/dev/null 2>&1 && [[ -n ${OMARCHY_PATH:-} ]]; then
  qmllint -I "$OMARCHY_PATH/shell" "$repo_root/BarWidget.qml" "$repo_root/Panel.qml"
fi
