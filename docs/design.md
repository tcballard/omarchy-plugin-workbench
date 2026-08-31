# Design

## Product boundary

Plugin Workbench is a local control plane for explicit plugin projects. It coordinates Omarchy's existing plugin commands rather than replacing the official registry or marketplace.

The native QML surface remains thin. The Rust helper owns filesystem boundaries, manifest parsing, process lifecycle, deployment receipts, and stable JSON responses.

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
