# Omarchy integration contract

Plugin Workbench is implemented as a native Quickshell panel loaded by its bar widget. It does not need a second GUI toolkit, process, or desktop window to behave as a first-class Omarchy surface.

## Stable panel toggle

The panel and its launcher share this module id:

```text
io.github.tcballard.plugin-workbench
```

The supported shell toggle is:

```bash
omarchy-shell shell toggle io.github.tcballard.plugin-workbench
```

## Proposed first-party binding

At the pinned Omarchy contract, `Super+Alt+P` is unclaimed by the stock bindings. A first-party integration can add this alongside the other native shell toggles:

```lua
o.bind(
  "SUPER + ALT + P",
  "Plugins",
  "omarchy-shell shell toggle io.github.tcballard.plugin-workbench"
)
```

Third-party installation must not edit `~/.config/hypr/bindings.lua`. Omarchy's schema-one plugin manifest does not declare global keybindings, and silently taking a system shortcut would make removal and conflict handling unsafe. Users can opt into the same binding in their personal override now; Omarchy can own the default if the panel is accepted and shipped first-party.

## Native inventory boundary

The Installed view treats `omarchy plugin list --json` as the authority for what exists and whether it is enabled. Workbench adds management provenance from local evidence:

| Classification | Evidence | Workbench actions |
| --- | --- | --- |
| Omarchy | `firstParty` from Omarchy | Status only unless Omarchy marks it disableable |
| Marketplace managed | Workbench ownership receipt | Reviewed update, repair, remove, enable/disable |
| Live development link | Plugin target is a symlink | Enable/disable; development actions remain in Build |
| Direct Git checkout | Plugin target contains `.git` | Reviewed fast-forward update, enable/disable |
| Local plugin | Other Omarchy-discovered plugin folder | Enable/disable |

This keeps discovery and enabled state under Omarchy while Workbench applies stronger, source-specific safety rules to mutations.
