#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"

jq -e '
  .schemaVersion == 1
  and .id == "io.github.tcballard.plugin-workbench"
  and (.kinds | index("bar-widget") != null)
  and .entryPoints.barWidget == "BarWidget.qml"
' "$repo_root/manifest.json" >/dev/null

jq -e '
  .["$schema"] == "https://json-schema.org/draft/2020-12/schema"
  and .properties.schemaVersion.const == 1
  and .properties.pluginPath.type == "string"
  and .properties.checks.maxItems == 32
  and .properties.workflows.maxItems == 32
  and .properties.environment.maxItems == 32
  and (.["$defs"].workflow.properties.capability.enum | index("preview") != null)
  and (.["$defs"].workflow.properties.capability.enum | index("publish") != null)
  and .["$defs"].check.properties.argv.maxItems == 64
  and .["$defs"].check.properties.timeoutSeconds.maximum == 1800
' "$repo_root/contracts/project-definition.schema.json" >/dev/null

jq -e '
  .["$schema"] == "https://json-schema.org/draft/2020-12/schema"
  and .properties.schemaVersion.const == 1
  and (.properties.result.enum | index("ready") != null)
  and (.properties.result.enum | index("needs-fixes") != null)
  and (.properties.result.enum | index("incomplete") != null)
  and (.properties.executableArtifacts.items["$ref"] == "#/$defs/artifact")
  and (.["$defs"].artifact.properties.status.enum | index("attested") != null)
  and (.["$defs"].fix.properties.result.enum | index("not-fixed") != null)
' "$repo_root/contracts/security-review.schema.json" >/dev/null

[[ -f $repo_root/BarWidget.qml ]]
[[ -f $repo_root/Panel.qml ]]
[[ -x $repo_root/bin/omarchy-plugin-workbench ]]

if command -v omarchy >/dev/null 2>&1; then
  omarchy plugin validate "$repo_root"
fi
if command -v qmllint >/dev/null 2>&1 && [[ -n ${OMARCHY_PATH:-} ]]; then
  qmllint -I "$OMARCHY_PATH/shell" "$repo_root/BarWidget.qml" "$repo_root/Panel.qml"
fi
