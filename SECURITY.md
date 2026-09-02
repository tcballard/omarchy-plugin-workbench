# Security model

Plugin Workbench treats plugin source, project configuration, shell logs, and installed targets as untrusted local input.

## Boundaries

- Projects are registered by an explicit canonical path. There is no automatic home-directory discovery.
- Plugin manifests must pass an internal schema-one validator. When the official `omarchy plugin validate` command is available, it must pass too.
- Plugin ids cannot use the reserved `omarchy.*` namespace or contain path separators/traversal.
- Entry points must be existing regular files beneath the plugin root.
- Symlinks and special files inside plugin packages are rejected.
- Configuration and receipt files are bounded, atomically replaced, and owner-only.
- The deployment target is always the exact manifest id beneath `~/.config/omarchy/plugins/`.
- Existing unmanaged files, directories, Git checkouts, and symlinks are never overwritten.
- Every managed switch verifies that the active symlink still matches the recorded deployment receipt.
- Snapshots omit `.git/` and `target/`, preserve executable intent, and use owner-only permissions.
- Project checks, environment probes, and workflows are imported as data but remain disabled until explicitly trusted.
- Executable trust is tied to the exact project-definition digest; editing the definition invalidates prior trust.
- Workflow capabilities require separate local approval and remain outside the shared project contract.
- Checks are exact argv arrays; no shell is inserted. Direct `sudo`, `doas`, `su`, and `pkexec` invocations are refused.
- Each check has a bounded timeout, a separate process group, null stdin, and 64 KiB stdout/stderr limits.
- Nested test sessions use a private temporary HOME and XDG persistent directories while retaining the host runtime directory needed to connect the nested compositor to Wayland.
- Nested Hyprland and Quickshell are separate owned process groups. Stop verifies each PID's Linux start time before signalling it, then erases only the recorded child beneath the Workbench test-session directory.
- Nested shell configuration disables discovered first-party plugins and idle locking, then enables only the registered project plugin. The plugin source remains a live link for development.
- The QML panel constructs process commands as arrays and never interpolates a project id or path into shell source.
- Update discovery examines at most 128 normal direct children of Omarchy's plugins directory, ignores symlinks and non-Git installs, invokes Git without a shell, and bounds command time and output.
- Updates require an explicit reviewed 40-character revision and confirmation. They refuse dirty, locally ahead, diverged, moved-local, and moved-remote states.
- A reviewed update fast-forwards only to the pinned object. Omarchy validation runs before the requested shell rescan; failure resets the checkout to its exact preceding revision, and rescan occurs only after validation succeeds.
- Marketplace refresh downloads one fixed HTTPS catalogue URL with redirect protocol, size, timeout, schema, production-mode, field, count, and duplicate-id bounds. The owner-only cache must remain a normal file.
- Marketplace installs accept only community root plugins with a GitHub HTTPS repository and full reviewed commit matching the current cache. Git hooks and filesystem monitors are disabled, the detached object is verified, the manifest id must match, and both internal and Omarchy validation precede publication.
- Marketplace ownership receipts are owner-only bounded files. Managed updates require the cached next reviewed commit, a clean receipt-matching checkout and fast-forward ancestry; validation failure resets the exact previous commit.
- Repair and uninstall require confirmation, refuse symlink targets and move normal owned directories into private recovery storage instead of deleting them.
- Release plans and submission drafts are inert owner-only artifacts. Workbench never executes their commands, creates tags, pushes commits, publishes releases or opens public issues.
- The bundled x86-64 helper is an intentional reviewed binary. Its Rust toolchain and container environment are pinned, Cargo input is frozen, and CI requires two clean builds plus the committed executable to be byte-identical before verifying its recorded SHA-256 and version.
- Trusted `main` and version-tag workflows publish GitHub build-provenance attestations binding the bundled executable digest to the repository, workflow, and exact commit. See `docs/reproducible-build.md`.
- The 0.2.0 command, workflow, worktree, nested-session, update, marketplace, and binary boundaries are recorded in `docs/security-audit-0.2.0.md`.

## Important limitation

Trusted project commands and enabled Omarchy plugins execute with the current user's permissions. A script can invoke its own shell, use the network, write outside the project, or access anything the user can access. Capability approval and trust are review boundaries, not confinement.

Update discovery contacts the configured Git remote. Git configuration and credentials are part of the user's existing trust boundary. Applying an update changes an installed checkout; filesystem watchers may observe those files before validation completes, and newly updated plugin code may run during shell rescan.

The marketplace catalogue is public network input authenticated by HTTPS, not a signed package index or a code audit. Its reviewed commit prevents Workbench from silently following a moving branch between review and install. Cloning and validation still process untrusted Git data and plugin files, and enabling an installed plugin executes third-party code as the user.

The disposable test window has the same limitation. It is a nested compositor with configuration/state separation, not a VM, container, user namespace, seccomp policy, or permission sandbox. It shares the user's kernel identity and may reach host files, network, D-Bus, PipeWire, secrets available to the user, and other session services. A malicious plugin can also deliberately escape its assigned process group. Test only code you are willing to run as your user.

## Non-goals

Workbench does not:

- Elevate privileges.
- Install system packages or systemd services.
- Execute plugin install/update hooks.
- Schedule or apply unattended updates.
- Publish, submit, tag, or push repositories.
- Delete project checkouts.
- Delete session worktrees or branches when a session is closed.
- Clean old snapshots automatically.
- Claim to security-audit third-party plugin behavior.

## Reporting

Do not include credentials, private repository contents, or sensitive shell logs in a public report. Report a vulnerability through the repository owner's private security channel once the project has a public remote.
