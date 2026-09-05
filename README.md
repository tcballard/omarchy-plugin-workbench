# Plugin Workbench for Omarchy

Plugin Workbench is a native Omarchy Quattro bar panel plus a bounded Rust helper for discovering, installing, developing, and managing shell plugins.

On the proposed first-party integration, install the helper package and launch Workbench with its native binding:

```bash
omarchy-pkg-add omarchy-plugin-workbench
# Super + Alt + P
```

For development against this repository, build the helper and link the QML module through Omarchy's plugin loader:

```bash
cargo build --workspace --locked --release
omarchy plugin add "$(pwd)" --enable
```

Workbench deliberately leaves its project registry and deployment history behind. To remove that retained local data too, run:

```bash
rm -rf -- "$HOME/.config/omarchy/plugin-workbench" \
  "$HOME/.local/state/omarchy/plugin-workbench"
```

The native panel is organised around the complete plugin lifecycle:

- **Discover** searches the cached official marketplace catalogue and installs reviewed snapshots.
- **Installed** inventories every plugin Omarchy discovers, identifies how it is managed, enables or disables eligible plugins, and offers only safe updates for its source type.
- **Updates** separates exact-revision review and application from everyday inventory management.
- **Build** creates, links, validates, tests, and prepares personal plugin projects.

Build supports these development loops explicitly:

- **New plugin** creates a validated personal Panel, bar widget, or service starter at an explicit absolute path, initializes Git, and registers it without committing or replacing existing files.
- **Live link** points Omarchy at the mutable plugin checkout for the fastest edit/reload loop.
- **Snapshot** copies the plugin into an immutable, content-addressed deployment and atomically switches Omarchy to it. Previous managed deployments remain available for rollback.
- **Test window** launches a disposable nested Hyprland compositor with an isolated Omarchy shell configuration and a live link to the project.

The workbench does not scan your home directory, execute install hooks, invoke a shell for project checks, use `sudo`, publish plugins, or replace an installation it did not create. Marketplace discovery uses the official published catalogue; update discovery is limited to normal Git checkouts directly beneath Omarchy's documented plugins directory.

## Status

The `0.3.0` line turns the established Workbench into a focused plugin management suite with automated Rust and lifecycle coverage. It is pinned to:

- Omarchy Quattro contract: `b686ed892d9c3020c3336203f6d34cc75b544e2b`
- Omarchy plugin manifest schema: `1`
- Rust: `1.98.0`
- Runtime architecture: official Omarchy `x86_64`

Native visual and interaction acceptance on a real Omarchy Quattro desktop remains a release gate. See [docs/live-acceptance.md](docs/live-acceptance.md).

The helper is reproducible from a pinned Rust/container environment and release candidates receive build-provenance attestations on version tags. See [the reproduction procedure](docs/reproducible-build.md) and [the 0.2.0 trust-boundary audit](docs/security-audit-0.2.0.md).

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

Available actions include Validate, Security review, Test, Test window, capability workflows, environment diagnostics, isolated sessions, handoffs, evidence, release readiness, Live link, Snapshot, Rollback, Enable, Disable, Undeploy, reviewed installed-plugin updates, Logs, and Doctor.

Results are compact rows showing a name and short status. Enter or click opens the selected row's details; only one detail view is loaded at a time. Escape collapses details before returning to the section rail and closing the panel.

On the section rail, Left/Right (or H/L) switches sections and Down or Enter enters the controls. Inside a section, arrows move to nearby controls in the direction pressed. Up/Down traverses selected result rows and scrolls them into view; Enter on an expanded row focuses its actions before activating anything. Brackets switch sections from non-editing controls. In text fields, Left/Right edits text and Up/Down moves between controls. Tab and Shift+Tab switch neighbouring shell panels.

Opening the panel uses the saved catalogue and retains loaded views for the shell
session. Use **Refresh** to fetch catalogue changes or reload local inventory,
and **Check updates** to fetch Git remotes. A fresh installation prompts you to
download the catalogue rather than fetching it on open. Tabs remain navigable
during operations; conflicting actions stay disabled. Successful mutations
invalidate cached views and previous Git update reviews.

## Launch the native plugin panel

Workbench is already a native Omarchy/Quickshell panel rather than a separate desktop application. Its stable shell module id is `io.github.tcballard.plugin-workbench`. Until Omarchy can ship it first-party, add this optional shortcut to `~/.config/hypr/bindings.lua`:

```lua
local o = require("omarchy")

o.bind(
  "SUPER + ALT + P",
  "Plugins",
  "omarchy-shell shell toggle io.github.tcballard.plugin-workbench"
)
```

The plugin does not rewrite global keybindings during installation. The current schema-one Omarchy plugin manifest has no keybinding declaration, so the stock `Super+Alt+P` binding belongs in Omarchy itself if Workbench is accepted as a first-party panel. See [the upstream integration contract](docs/omarchy-integration.md).

## Create a personal plugin

Use **New plugin** in the panel, or create and register the same safe starter from the CLI:

```bash
bin/omarchy-plugin-workbench new /absolute/path/my-plugin \
  --id io.github.you.my-plugin \
  --name "My Plugin" \
  --kind panel
```

`panel` creates both a panel and its bar-widget launcher. `bar-widget` and `service` create focused single-entry-point starters. Workbench stages and validates the complete tree before publishing it, refuses an existing destination, initializes a `main` Git repository when Git is available, and never creates a commit or runs plugin code.

## Search and install official marketplace listings

Open **Discover** in the panel to refresh the official catalogue, search locally by name, description, author, category, kind, or tag, and filter built-in, verified, installable, or installed listings. “Official marketplace” describes the catalogue source; community listings are not presented as Omarchy-authored plugins. Built-ins are browse-only because Omarchy manages them.

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
bin/omarchy-plugin-workbench installed
bin/omarchy-plugin-workbench installed-disable io.github.example.plugin
bin/omarchy-plugin-workbench installed-enable io.github.example.plugin
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
target/release/omarchy-plugin-workbench --version

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

## Exact-commit security review

Security review is a separate lifecycle stage from executable validation and testing. Workbench prepares a private brief and bounded static inventory at one clean, exact Git commit without running plugin code, tests, builds, examples, installers, hooks, workflows, privileged commands, or bundled binaries:

```bash
bin/omarchy-plugin-workbench security-review-prepare io.github.example.plugin
```

The generated brief applies the Omarchy marketplace maintainer method across process execution, filesystem boundaries, network and external content, QML sinks, IPC and privileges, secrets, agent/tool configuration, dependencies, workflows, releases, updates, and executable provenance. Its inventory is navigation evidence, not a clean scan or safety conclusion.

The reviewer returns one schema-one JSON object in their final response. After inspecting it, import it with an explicit manual-review confirmation:

```bash
bin/omarchy-plugin-workbench security-review-import io.github.example.plugin \
  --file /path/to/review.json --confirm-manual-review
bin/omarchy-plugin-workbench security-review-status io.github.example.plugin
```

Workbench accepts `ready`, `needs-fixes`, or `incomplete` and derives `stale` whenever the worktree becomes dirty or moves away from the reviewed commit. A `Ready` report cannot retain blockers or unresolved critical/high/medium findings, and it must account for every detected executable artifact with reviewable source or provenance evidence. The report contract is published at [`contracts/security-review.schema.json`](contracts/security-review.schema.json).

After remediation, prepare a fix-verification brief against the latest imported findings and the new exact commit:

```bash
bin/omarchy-plugin-workbench security-review-prepare io.github.example.plugin --verify-fixes
```

Review evidence remains inspectable instead of being collapsed into one status flag. History preserves every imported exact-commit result, while `show` returns the complete latest report or the latest report at a named full revision:

```bash
bin/omarchy-plugin-workbench security-review-history io.github.example.plugin
bin/omarchy-plugin-workbench security-review-show io.github.example.plugin
bin/omarchy-plugin-workbench security-review-show io.github.example.plugin \
  --revision FULL_40_CHARACTER_REVIEWED_COMMIT
```

When a current review has findings, Workbench can turn all findings—or selected ids—into one private remediation brief and isolated Git worktree. The source checkout remains untouched and publication is excluded from the generated objective:

```bash
bin/omarchy-plugin-workbench security-remediation-start io.github.example.plugin \
  --finding SEC-001 --agent codex
```

After a current review reaches `Ready`, generate a shareable Markdown and JSON dossier containing the plugin identity, exact reviewed commit, review-record digest, findings, executable provenance and current-revision Workbench evidence:

```bash
bin/omarchy-plugin-workbench security-review-dossier io.github.example.plugin
```

The dossier is written to private state and is not uploaded or attached automatically. It becomes stale with the review as soon as the source moves.

Release and marketplace submission preparation require a current `Ready` manual review. Workbench records the result as evidence but does not describe it as certification, warranty, marketplace approval, or a replacement for maintainer review.

## Release and marketplace submission preparation

`release-plan` converts passing readiness evidence into an owner-only JSON plan containing the exact current revision, tag and reviewable argv arrays. It does not execute them. `submission-prepare` validates the root layout, README, licence, category, one-to-three official tags, cached ID/repository collisions and explicit confirmation of the five official checklist statements, then writes the current official issue body and matching exact-commit security dossier without creating a public issue.

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
| `~/.local/state/omarchy/plugin-workbench/security-reviews/` | Review history, remediation briefs, and exact-commit dossiers |
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
