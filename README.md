# Plugin Workbench for Omarchy

Plugin Workbench is a native Omarchy Quattro bar panel plus a bounded Rust helper for discovering, installing, developing, and managing shell plugins.

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
- **Test window** launches a disposable nested Hyprland compositor with an isolated Omarchy shell configuration and a live link to the project.

The workbench does not scan your home directory, execute install hooks, invoke a shell for project checks, use `sudo`, publish plugins, or replace an installation it did not create. Marketplace discovery uses the official published catalogue; update discovery is limited to normal Git checkouts directly beneath Omarchy's documented plugins directory.

## Status

This repository is a complete `0.2.0` implementation with automated Rust and lifecycle coverage. It is pinned to:

- Omarchy Quattro contract: `b686ed892d9c3020c3336203f6d34cc75b544e2b`
- Omarchy plugin manifest schema: `1`
- Rust: `1.98.0`
- Runtime architecture: official Omarchy `x86_64`

Native visual and interaction acceptance on a real Omarchy Quattro desktop remains a release gate. See [docs/live-acceptance.md](docs/live-acceptance.md).

The bundled x86-64 helper is byte-reproducible from a pinned Rust/container environment and receives GitHub build-provenance attestations on trusted `main` and version-tag builds. See [the reproduction procedure](docs/reproducible-build.md) and [the 0.2.0 trust-boundary audit](docs/security-audit-0.2.0.md).

## What it manages

For each explicitly registered project, Workbench shows:

- Git revision and dirty state.
- Plugin source and project paths.
- Current deployment mode.
- Deployed revision.
- Omarchy enabled/disabled state.
- Whether project-defined checks are trusted.
- Declared capability workflows, active Git work sessions, and disposable nested test sessions.
- Whether the project definition changed after it was trusted.
- Drift when the managed link changes outside Workbench.

Available actions include Validate, Test, Test window, capability workflows, environment diagnostics, isolated sessions, handoffs, evidence, release readiness, Live link, Snapshot, Rollback, Enable, Disable, Undeploy, reviewed installed-plugin updates, Logs, and Doctor.

## Search and install official marketplace listings

Open **Marketplace** in the panel to refresh the official catalogue, search locally by name, description, author, category, kind, or tag, and filter built-in, verified, or installable listings. “Official marketplace” describes the catalogue source; community listings are not presented as Omarchy-authored plugins. Built-ins are browse-only because Omarchy manages them.

Workbench caches [`https://omarchyplugins.com/catalog.json`](https://omarchyplugins.com/catalog.json) only when you explicitly refresh it. Search then works against that private local cache without a network request:

```bash
bin/omarchy-plugin-workbench marketplace-refresh
bin/omarchy-plugin-workbench marketplace-search clipboard --verified --json
bin/omarchy-plugin-workbench marketplace-search --category Development --installable
```

For an installable community root plugin, search returns its repository and full marketplace-reviewed commit. Installation requires those exact values plus confirmation; Workbench rejects a stale review, checks out the detached commit with Git hooks disabled, validates the manifest internally and through Omarchy, then publishes and optionally enables it:

```bash
bin/omarchy-plugin-workbench marketplace-install io.github.example.plugin \
  --repo https://github.com/example/plugin \
  --revision FULL_40_CHARACTER_REVIEWED_COMMIT \
  --enable --yes
```

The catalogue is public network input protected by HTTPS, not a signed package index. A reviewed commit limits moving-target risk but does not make third-party code safe; enabling a plugin runs it with your user permissions.

Workbench records every installation it creates. **Installed** shows the complete host view; **Workbench managed** listings can be updated only to the catalogue's next exact reviewed commit, repaired from that reviewed snapshot, or uninstalled with a recovery copy retained in private state. Marketplace-managed plugins are intentionally excluded from the separate mutable-remote update path.

```bash
bin/omarchy-plugin-workbench portfolio
bin/omarchy-plugin-workbench marketplace-managed
bin/omarchy-plugin-workbench marketplace-update io.github.example.plugin \
  --revision FULL_REVIEWED_COMMIT --yes
bin/omarchy-plugin-workbench marketplace-repair io.github.example.plugin --yes
bin/omarchy-plugin-workbench marketplace-uninstall io.github.example.plugin --yes
```

## Review and apply installed plugin updates

**Check updates** fetches `origin/HEAD` for normal Git checkouts directly beneath `~/.config/omarchy/plugins`. The panel shows incoming commit subjects, the revision transition, and a bounded diff stat. Dirty, locally ahead, diverged, and failed-fetch checkouts are visible but never offered for automatic application. Live development links and non-Git installations are ignored.

An update is pinned to the full revision shown during review. Workbench refuses it if either the local checkout or remote revision changes before application. A confirmed update fast-forwards to that exact object, runs `omarchy plugin validate`, rolls back to the preceding revision on validation failure, and asks `omarchy-shell` to rescan after success.

The panel's **Update** and **Update all** buttons carry the reviewed revisions automatically. The CLI keeps that decision explicit:

```bash
bin/omarchy-plugin-workbench updates --json
bin/omarchy-plugin-workbench update io.github.example.plugin \
  --revision FULL_40_CHARACTER_OBJECT_ID --yes
bin/omarchy-plugin-workbench update-all \
  --reviewed io.github.example.one=FULL_OBJECT_ID \
  --reviewed io.github.example.two=FULL_OBJECT_ID \
  --yes
```

Fetching and updating may contact plugin remotes. Updated plugin code still runs with your user permissions; review code you trust before applying it. Workbench does not schedule unattended updates.

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

## Disposable nested test window

On an active Omarchy/Hyprland desktop, launch the registered plugin in a second Hyprland compositor running inside a normal window:

```bash
bin/omarchy-plugin-workbench test-session-start io.github.example.plugin
bin/omarchy-plugin-workbench test-sessions io.github.example.plugin
bin/omarchy-plugin-workbench test-session-stop io.github.example.plugin
```

The panel exposes the same lifecycle as **Test window** / **Stop test window**. The nested shell gets a private temporary HOME plus private XDG config, cache, state, and data directories. Its `shell.json` disables first-party non-bar plugins, turns off idle locking, and enables the project plugin. Bar widgets and replacement bars appear in the nested bar; panels, overlays, and menus are summoned after startup. The project source is live-linked, so Quickshell sees edits while the window is open.

Stopping the session terminates the owned Quickshell and Hyprland process groups, verifies their Linux process start times to avoid killing reused PIDs, and erases the temporary tree. `release-check` blocks while a nested test session is active.

This is process and configuration isolation, not a VM, container, or security sandbox. Plugin code runs as your user and retains access to the host filesystem, network, session D-Bus, audio, and other user services. Use it to protect the active desktop from accidental shell/config breakage—not to execute untrusted code safely.

If a plugin lives in a subdirectory that is not `omarchy-plugin/`, specify it:

```bash
bin/omarchy-plugin-workbench add /path/to/project --plugin-path packaging/omarchy
```

## Portable project contract

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
  ],
  "environment": [
    {
      "name": "rust",
      "argv": ["rustc", "--version"],
      "required": true
    }
  ],
  "workflows": [
    {
      "name": "preview",
      "capability": "preview",
      "argv": ["./scripts/preview"],
      "timeoutSeconds": 600,
      "requires": []
    }
  ]
}
```

This file contains shared project facts, never agent-specific configuration. Codex, OpenCode, or another Agent Skills host can operate on the same checkout and contract.

Registration reads declared commands but does not trust or run them. Review the file, then make trust and workflow-capability decisions explicitly:

```bash
bin/omarchy-plugin-workbench trust io.github.example.plugin
bin/omarchy-plugin-workbench check io.github.example.plugin
bin/omarchy-plugin-workbench environment io.github.example.plugin
bin/omarchy-plugin-workbench approve io.github.example.plugin preview
bin/omarchy-plugin-workbench workflow io.github.example.plugin preview
```

Trust is bound to the exact SHA-256 of `.omarchy-workbench.json`; editing it invalidates executable trust. Commands run without an inserted shell, with null stdin, a fresh process group, a declared timeout, and bounded captured output. They still execute with your user permissions and must be treated as arbitrary project code. Capability approval is a local policy gate, not an operating-system sandbox.

After intentionally editing the definition, reload it before reviewing and trusting the new command set. Refresh revokes prior command trust and every capability approval:

```bash
bin/omarchy-plugin-workbench refresh io.github.example.plugin
```

## Parallel agent sessions and handoffs

Create one isolated Git worktree and `codex/*` branch per task. The optional agent label is local session metadata only and never enters the project contract:

```bash
bin/omarchy-plugin-workbench session-start io.github.example.plugin \
  --task repair-preview --agent opencode \
  --objective "Repair preview startup without changing the plugin contract"
bin/omarchy-plugin-workbench sessions io.github.example.plugin
```

Workbench refuses to create a session from a dirty source checkout. Closing a session is deliberately non-destructive: it marks the record closed but retains the branch and worktree.

```bash
bin/omarchy-plugin-workbench handoff SESSION_ID \
  --decision "Keep startup agent-neutral" \
  --next-action "Run project checks"
bin/omarchy-plugin-workbench session-close SESSION_ID
```

Handoffs capture the objective, decisions, blockers, next action, branch, worktree, revision, and dirty state in private local state.

## Evidence and release readiness

Checks, workflows, environment probes, and release preflights append bounded structured records to the per-project evidence ledger. Release readiness is read-only: it checks validation, clean Git state, changelog/version agreement, clean passing evidence for the current revision, and open sessions. It never tags, pushes, publishes, or closes sessions.

```bash
bin/omarchy-plugin-workbench diagnose io.github.example.plugin
bin/omarchy-plugin-workbench evidence io.github.example.plugin --limit 20
bin/omarchy-plugin-workbench release-check io.github.example.plugin
```

## Release and marketplace submission preparation

`release-plan` converts passing readiness evidence into an owner-only JSON plan containing the exact current revision, tag and reviewable argv arrays. It does not execute them. `submission-prepare` validates the root layout, README, licence, category, one-to-three official tags, cached ID/repository collisions and explicit confirmation of the five official checklist statements, then writes the current official issue body without creating a public issue.

```bash
bin/omarchy-plugin-workbench release-plan io.github.example.plugin
bin/omarchy-plugin-workbench submission-prepare io.github.example.plugin \
  --repo https://github.com/example/plugin \
  --category "Developer Tools" --tag quickshell --tag bar \
  --confirm-checklist
```

The final tag, push, GitHub release and marketplace issue remain explicit public actions. This prevents a local panel click or compromised project file from publishing under your identity.

## State and recovery

| Path | Purpose |
| --- | --- |
| `~/.config/omarchy/plugin-workbench/projects.json` | Explicit project registry |
| `~/.local/state/omarchy/plugin-workbench/snapshots/` | Immutable plugin snapshots |
| `~/.local/state/omarchy/plugin-workbench/deployments/` | Deployment history and active receipt |
| `~/.local/state/omarchy/plugin-workbench/sessions/` | Isolated task worktrees |
| `~/.local/state/omarchy/plugin-workbench/sessions.json` | Local session ownership and lifecycle |
| `~/.local/state/omarchy/plugin-workbench/test-sessions/` | Disposable nested compositor homes, config, logs, and ownership records |
| `~/.local/state/omarchy/plugin-workbench/handoffs/` | Structured continuation records |
| `~/.local/state/omarchy/plugin-workbench/evidence/` | Append-only per-project evidence ledgers |
| `~/.local/state/omarchy/plugin-workbench/marketplace/catalog.json` | Explicitly refreshed official catalogue cache |
| `~/.local/state/omarchy/plugin-workbench/marketplace/receipts/` | Workbench marketplace ownership and reviewed revisions |
| `~/.local/state/omarchy/plugin-workbench/marketplace/trash/` | Recoverable repair/uninstall checkouts |
| `~/.local/state/omarchy/plugin-workbench/publishing/` | Reviewable release plans and submission drafts |
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
