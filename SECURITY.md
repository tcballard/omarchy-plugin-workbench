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
- Project checks are imported as data but remain disabled until explicitly trusted.
- Checks are exact argv arrays; no shell is inserted. Direct `sudo`, `doas`, `su`, and `pkexec` invocations are refused.
- Each check has a bounded timeout, a separate process group, null stdin, and 64 KiB stdout/stderr limits.
- The QML panel constructs process commands as arrays and never interpolates a project id or path into shell source.
- The bundled x86-64 helper is an intentional reviewed binary. CI builds and tests the locked source, then independently verifies the committed executable's recorded SHA-256 and version. Native linker output is not claimed to be reproducible across build environments.

## Important limitation

Trusted project checks and enabled Omarchy plugins execute with the current user's permissions. A script can invoke its own shell or access anything the user can access. Trust is therefore a review boundary, not confinement.

## Non-goals

Workbench does not:

- Elevate privileges.
- Install system packages or systemd services.
- Execute plugin install/update hooks.
- Publish, submit, tag, or push repositories.
- Delete project checkouts.
- Clean old snapshots automatically.
- Claim to security-audit third-party plugin behavior.

## Reporting

Do not include credentials, private repository contents, or sensitive shell logs in a public report. Report a vulnerability through the repository owner's private security channel once the project has a public remote.
