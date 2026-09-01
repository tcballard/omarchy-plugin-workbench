# Design

## Product boundary

Plugin Workbench is a local control plane for explicit plugin projects. It coordinates Omarchy's existing plugin commands rather than replacing the official registry or marketplace.

The native QML surface remains thin. The Rust helper owns filesystem boundaries, manifest parsing, process lifecycle, deployment receipts, and stable JSON responses.

## Official marketplace discovery

Discovery consumes the marketplace's published production `catalog.json`, not repository scraping or free-form install commands. Refresh is explicit and bounded; searches run entirely against the private cache. Results preserve the distinction between Omarchy built-ins and community catalogue entries.

Built-ins are browse-only. A community listing is actionable only when the catalogue marks it installable, its repository is a root plugin on GitHub HTTPS, and it carries a full listing-validated commit. The panel carries the displayed repository and commit back to the helper. The helper compares both with the current cache, performs a detached checkout with hooks disabled, verifies `HEAD`, validates the manifest id and plugin contract, then atomically publishes the directory and asks Omarchy to rescan. This avoids executing marketplace-provided command strings and rejects a listing that changed after review.

Successful installs create a separate ownership receipt. Only a normal directory whose manifest and Git revision still match that receipt is eligible for marketplace-managed mutation. A newer catalogue snapshot is applied only as an exact fast-forward and is rolled back on validation failure. Repair and uninstall retain the displaced owned directory under private trash; symlink or special-file drift is never followed or replaced.

## Publishing boundary

Release planning consumes current release-readiness evidence and emits exact inert argv arrays. Submission preparation mirrors the current official form and gates root layout, documentation, licence, taxonomy, cached collisions and owner checklist confirmation. These artifacts stop at the public-identity boundary: Workbench does not run tag/push/release commands or submit issues.

## Installed plugin updates

Update management is independent of the explicit development-project registry. Discovery is restricted to bounded, normal Git checkouts immediately beneath Omarchy's documented plugins directory; live links and non-Git installs remain outside this path.

The review step fetches `origin/HEAD`, classifies the checkout as up to date, update available, dirty, locally ahead, diverged, or failed, and returns bounded incoming commit and diff-stat evidence. Only a clean fast-forward is actionable.

Application carries the reviewed full object id back into the helper. The helper fetches and classifies again, rejects remote or local movement, and merges the exact reviewed object rather than a moving branch name. It then delegates contract checking to `omarchy plugin validate`, resets to the exact preceding revision on validation failure, and invokes one shell rescan after a successful batch. There is no timer, background service, or unattended policy.

## Deployment states

| State | Meaning |
| --- | --- |
| `not-deployed` | No plugin exists at the manifest-id target |
| `live-link` | The target points directly to the mutable plugin root |
| `snapshot` | The target points to an immutable Workbench snapshot |
| `drifted` | A receipt exists but the target no longer matches it |
| `unmanaged-link` | A symlink exists without a Workbench receipt |
| `unmanaged-install` | A normal directory or checkout exists |

An action can replace only `not-deployed`, `live-link`, or `snapshot`. Drift and unmanaged states require manual inspection.

## Atomic switch

Workbench creates a temporary sibling symlink and renames it over the managed target. The receipt is updated only after the filesystem switch succeeds. A crash between those operations is detected as drift rather than guessed away.

## Snapshot identity

Snapshot names contain the deployment timestamp and the first twelve characters of a SHA-256 digest over sorted relative paths and file contents. `.git/` and `target/` are excluded. Snapshot names are evidence, not a cryptographic trust claim; the full deployment receipt records the Git revision and dirty state separately.

## Rollback

Receipts keep an ordered deployment history and an active index. Rollback moves the index one entry backwards and atomically redirects the target. Deploying after a rollback truncates the abandoned forward branch before appending the new deployment.

## Shared contract and local policy

`.omarchy-workbench.json` is declarative, agent-neutral project input. It contains plugin location, checks, environment probes, and named capability workflows. Agent labels, approvals, sessions, handoffs, and evidence remain in owner-only local state.

Workbench stores the project-definition digest at registration and the trusted digest at approval. A byte change invalidates executable trust. Workflows also require local approval for their declared capability and requirements.

The command runner avoids shell parsing, but it cannot make trusted code safe. Its timeout, process-group termination, output bounds, and null stdin prevent common hangs and accidental UI flooding.

## Parallel work and evidence

Each task session owns a `codex/*` branch and Git worktree beneath private Workbench state. The source checkout must be clean before a session starts. Closing is metadata-only and retains the worktree and branch, keeping recovery explicit.

Structured handoffs describe work rather than a particular agent protocol. Evidence records bind an operation result to revision, dirty state, platform, and time. Release readiness consumes that evidence but performs no release mutation.

## Disposable nested runtime

A test session launches `Hyprland --config` as a Wayland client of the active desktop, discovers the new compositor by its exact PID through `hyprctl instances -j`, and launches a separate Quickshell process against that compositor's socket. No Omarchy autostart file is loaded.

The session owns a private tree beneath `test-sessions/`: HOME, XDG persistent state, generated Hyprland config, minimal `shell.json`, a bounded-lifetime log, and a process-identity receipt. The host `XDG_RUNTIME_DIR` remains in use because the nested compositor must connect to the outer Wayland socket. The project plugin is the only live link inside the temporary HOME.

Each child is a process-group leader. Shutdown compares both `/proc/<pid>/stat` start times with the receipt before sending TERM and, after a bounded grace period, KILL. Cleanup refuses the parent directory and symlink roots. The whole session tree is removed after stop, so crash logs are intentionally ephemeral unless copied before stopping.

This boundary protects the active Omarchy shell and its configuration from accidental development failures. It does not confine same-user plugin behavior.

## Authoring companion boundary

Build Omarchy Plugins is a separately versioned Agent Plugin, not an Omarchy
runtime dependency. The two products integrate only through the versioned
`.omarchy-workbench.json` project-definition contract. Workbench does not embed
the companion skills, install them, invoke an agent host, or automatically trust
generated checks.

Companion discovery is evidence-bounded. `doctor` reads only managed installer
receipts at the documented Agent Skills locations beneath the user's home
directory. Normal panel refresh and project registration perform no companion
scan.
