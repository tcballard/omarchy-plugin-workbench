# Plugin Workbench for Omarchy

Plugin Workbench is a native Omarchy Quattro bar panel plus a bounded Rust helper for developing and managing local shell plugins.

Install it directly from GitHub:

```bash
omarchy plugin add https://github.com/tcballard/omarchy-plugin-workbench.git --enable
```

Remove the plugin with:

```bash
omarchy plugin remove io.github.tcballard.plugin-workbench
```

Workbench deliberately leaves its project registry and deployment history behind. To remove that retained local data too, run:

```bash
rm -rf -- "$HOME/.config/omarchy/plugin-workbench" \
  "$HOME/.local/state/omarchy/plugin-workbench"
```

It answers two different development needs explicitly:

- **Live link** points Omarchy at the mutable plugin checkout for the fastest edit/reload loop.
- **Snapshot** copies the plugin into an immutable, content-addressed deployment and atomically switches Omarchy to it. Previous managed deployments remain available for rollback.

The workbench does not scan your home directory, execute install hooks, invoke a shell for project checks, use `sudo`, publish plugins, or replace an installation it did not create.

## Status

This repository is a complete `0.1.0` implementation with automated Rust and lifecycle coverage. It is pinned to:

- Omarchy Quattro contract: `b686ed892d9c3020c3336203f6d34cc75b544e2b`
- Omarchy plugin manifest schema: `1`
- Rust: `1.98.0`
- Runtime architecture: official Omarchy `x86_64`

Native visual and interaction acceptance on a real Omarchy Quattro desktop remains a release gate. See [docs/live-acceptance.md](docs/live-acceptance.md).

## What it manages

For each explicitly registered project, Workbench shows:

- Git revision and dirty state.
- Plugin source and project paths.
- Current deployment mode.
- Deployed revision.
- Omarchy enabled/disabled state.
- Whether project-defined checks are trusted.
- Drift when the managed link changes outside Workbench.

Available actions are Validate, Test, Live link, Snapshot, Rollback, Enable, Disable, Undeploy, Logs, and Doctor.

## Build the local package

```bash
cargo build --workspace --locked --release
sha256sum --check bin/SHA256SUMS
bin/omarchy-plugin-workbench-x86_64 --version

omarchy plugin validate .
```

## Load it while developing

From the repository root on an Omarchy machine:

```bash
PLUGIN_ID="io.github.tcballard.plugin-workbench"
PLUGIN_TARGET="$HOME/.config/omarchy/plugins/$PLUGIN_ID"

ln -s "$PWD" "$PLUGIN_TARGET"
omarchy-shell shell rescanPlugins
omarchy plugin enable "$PLUGIN_ID" --section right
```

The native panel can register a project by absolute path. The CLI exposes the same workflow:

```bash
bin/omarchy-plugin-workbench add /absolute/path/to/a/plugin-project
bin/omarchy-plugin-workbench list
bin/omarchy-plugin-workbench validate io.github.example.plugin
bin/omarchy-plugin-workbench link io.github.example.plugin
bin/omarchy-plugin-workbench snapshot io.github.example.plugin
bin/omarchy-plugin-workbench rollback io.github.example.plugin
```

If a plugin lives in a subdirectory that is not `omarchy-plugin/`, specify it:

```bash
bin/omarchy-plugin-workbench add /path/to/project --plugin-path packaging/omarchy
```

## Project-defined checks

A project can declare exact argument vectors in `.omarchy-workbench.json`:

```json
{
  "schemaVersion": 1,
  "pluginPath": "omarchy-plugin",
  "checks": [
    {
      "name": "complete",
      "argv": ["cargo", "test", "--workspace", "--locked"],
      "timeoutSeconds": 600
    }
  ]
}
```

Registration reads these commands but does not trust or run them. Review the file, then make the trust decision explicitly:

```bash
bin/omarchy-plugin-workbench trust io.github.example.plugin
bin/omarchy-plugin-workbench check io.github.example.plugin
```

Checks run without a shell, with null stdin, a fresh process group, a declared timeout, and bounded captured output. They still execute with your user permissions and must be treated as arbitrary project code.

## State and recovery

| Path | Purpose |
| --- | --- |
| `~/.config/omarchy/plugin-workbench/projects.json` | Explicit project registry |
| `~/.local/state/omarchy/plugin-workbench/snapshots/` | Immutable plugin snapshots |
| `~/.local/state/omarchy/plugin-workbench/deployments/` | Deployment history and active receipt |
| `~/.config/omarchy/plugins/<plugin-id>` | Atomic symlink controlled by Workbench |

Config, state, receipts, and captured check output use owner-only permissions. Snapshot directories are owner-only too.

Workbench refuses to replace a normal directory, a Git checkout installed by Omarchy, or an unrecognised symlink. `undeploy` removes only the managed symlink and retains snapshot history. It never deletes the source checkout.

## Build Omarchy Plugins companion

[Build Omarchy Plugins](https://github.com/tcballard/build-omarchy-plugins) is the
default authoring companion for Workbench. It designs, scaffolds, tests, and
prepares releases; Workbench registers the resulting local checkout and owns its
live-link, snapshot, rollback, and shell lifecycle.

Generated projects declare the versioned integration in
`.omarchy-workbench.json`. Workbench reads the proposed exact-argv checks at
registration but never trusts them automatically. Review the definition and use
the panel's **Trust checks** action or the CLI `trust` command before running
project code.

The authoritative schema is
[`contracts/project-definition.schema.json`](contracts/project-definition.schema.json).
`doctor --json` checks only the documented user-level Agent Skills receipt
locations for a managed companion installation and reports its version. It does
not search the rest of the home directory or install anything.

## CLI JSON contract

Pass `--json` anywhere in a command. Successful commands emit one JSON value to stdout. Failures emit:

```json
{"ok":false,"error":"bounded explanation"}
```

and exit non-zero. This is the interface used by the QML panel.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
bash -n bin/omarchy-plugin-workbench scripts/validate.sh
scripts/validate.sh
```

The integration suite creates a private fake home and verifies registration, live linking, snapshotting, atomic switching, rollback, undeployment, removal, and preservation of an unmanaged target.

Create a deterministic local handoff archive with:

```bash
scripts/package.sh
```

## Security

Omarchy plugins are unsandboxed code inside the long-running shell. Workbench reduces accidental mutation and command ambiguity; it is not a sandbox. Read [SECURITY.md](SECURITY.md) before adding execution features or installing third-party plugins.
