# Omarchy Quattro live acceptance

Complete this matrix on an actual official x86-64 Omarchy Quattro desktop before publishing `0.2.0`.

## Record the host

- Omarchy version and contract revision.
- Whether the host matches `b686ed892d9c3020c3336203f6d34cc75b544e2b`.
- Quickshell and Hyprland versions.
- Monitor count, layout, and scale factors.
- Plugin Workbench commit and release-binary SHA-256.

## Build and validate

```bash
cargo build --workspace --locked --release
install -m 0755 target/release/omarchy-discovery bin/omarchy-discovery-x86_64
omarchy plugin validate .
qmllint -I "$OMARCHY_PATH/shell" BarWidget.qml Panel.qml
```

## Native shell matrix

- Link this checkout under `~/.config/omarchy/plugins/io.github.tcballard.discovery`.
- Rescan and enable the widget in the right bar section.
- Confirm the bar label renders on horizontal and vertical bars.
- Open by click and `omarchy-shell shell summon io.github.tcballard.discovery '{}'`.
- Close with Escape, outside click, repeated widget click, and shell hide.
- Switch directly between Workbench and both neighbouring panels.
- Confirm keyboard focus returns to the previous application after closing.
- Confirm the panel fits at 100%, fractional, and high-DPI scale where available.
- Register valid root and nested plugin projects from the panel.
- Open Marketplace, refresh the production catalogue, and search by text plus each built-in, verified, and installable filter.
- Install and enable one reviewed community listing; confirm its displayed commit equals installed `HEAD` and its panel state changes to installed.
- Refresh after a newer reviewed snapshot, apply it, and confirm `HEAD`, the ownership receipt and panel status all move to the same exact commit.
- Repair a deliberately damaged managed checkout and confirm the prior directory remains recoverable in Workbench trash.
- Use the two-step uninstall control and confirm the target disappears, the receipt is removed and the recovery copy remains.
- Change the cached listing between review and install and confirm Workbench refuses it without creating a target.
- Verify malformed, reserved-id, symlinked, and missing-entry-point projects fail visibly.
- Run Validate and Test, including an untrusted-check refusal.
- Link a development checkout and confirm saved QML changes reload.
- Snapshot a dirty checkout and confirm the dirty marker and deployed revision remain truthful.
- Roll back snapshot → live and snapshot → snapshot.
- Enable and disable bar-widget, panel, overlay, menu, service, and bar plugin fixtures where practical.
- Replace a managed target externally and confirm Workbench reports drift and refuses mutation.
- Place an unmanaged checkout at the target and confirm Workbench preserves it.
- Restart `omarchy-shell` and confirm registrations, receipts, and deployment state survive.
- Lock the screen and verify Workbench does not perform plugin writes while locked.

## Failure and recovery

- Kill Workbench during a long check and confirm no child process remains.
- Force a check timeout and confirm the process group is terminated.
- Make config/state paths symlinks and confirm startup refuses them.
- Remove a historical snapshot and confirm rollback fails without changing the active target.
- Make `omarchy-shell` unavailable during a deployment and confirm the successful filesystem switch is reported with an explicit rescan warning.
- Undeploy and confirm only the managed link disappears; source and snapshots remain.

Do not mark live acceptance complete from a container, source review, `qmllint`, or automated tests alone.
