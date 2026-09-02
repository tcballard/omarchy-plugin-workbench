# Plugin Workbench 0.2.0 trust-boundary audit

Scope: the command/workflow runner, Git worktree handoffs, nested Hyprland test
sessions, installed-plugin updates, marketplace lifecycle, and bundled runtime.
This review is pinned to the final 0.2.0 release commit in the marketplace issue.

## Findings

No known exploitable finding remains from this bounded review. The controls below
reduce accidental or confused-deputy behavior; they do not sandbox explicitly
trusted project code or enabled plugins.

| Boundary | Reviewed control | Regression evidence | Residual risk |
| --- | --- | --- | --- |
| Project commands and workflows | Definitions are bounded data, disabled until explicit local trust, and trust is revoked by any definition-byte change. Commands are exact argv arrays; Workbench inserts no shell and rejects direct privilege tools. | `project_checks_require_an_explicit_trust_decision`; `workflows_are_capability_gated_and_definition_changes_revoke_trust`; process unit tests | A trusted executable can launch a shell, use the network, or access everything available to the user. |
| Process lifetime and output | Null stdin, separate process groups, bounded time, TERM/KILL cleanup, and 64 KiB stdout/stderr caps. | process timeout/output unit tests and lifecycle check evidence | Deliberately detached descendants can escape a process group. |
| Agent sessions and worktrees | A clean registered checkout is required. Workbench creates a new namespaced branch/worktree beneath private state; close is metadata-only and never deletes the worktree or branch. Handoffs record revision and dirty state without executing agent output. | `isolated_sessions_and_handoffs_remain_agent_neutral` | Git hooks and configured credentials remain part of the user's Git trust boundary. |
| Nested Hyprland session | Private HOME/XDG state, generated minimal config, only the selected plugin linked, owned process groups, PID start-time checks, and symlink-safe bounded cleanup. | test-session cleanup, symlink and PID-reuse unit tests; native Quattro acceptance | This is isolation for state and lifecycle, not a VM or security sandbox; host user access remains. |
| Generic installed-plugin updates | Only normal direct child Git checkouts are considered. Symlinks and marketplace-managed installs are excluded. Dirty, ahead, diverged or moved revisions are blocked. Apply requires the reviewed full SHA and fast-forward ancestry; failed validation resets the exact prior commit before rescan. | `update_review_reports_the_pinned_revision_commits_and_diff_stat`; `dirty_and_live_link_plugins_are_never_offered_for_update`; `update_applies_only_the_reviewed_revision_then_validates_and_rescans`; `update_refuses_stale_review_and_rolls_back_failed_validation` | Git fetch contacts the configured remote; filesystem watchers may observe files before validation completes. |
| Marketplace install/update/repair/uninstall | HTTPS GitHub root repo, exact catalogue-reviewed SHA, hooks/monitors disabled, manifest ID match, internal and Omarchy validation, owner-only bounded receipts, clean receipt-matching updates, and recoverable trash. Symlink/special target drift is refused. | marketplace install/cache tests plus receipt/update/repair/uninstall and symlink-drift lifecycle regressions | The HTTPS catalogue and reviewed commit are not a code audit; enabled third-party plugins run as the user. |
| Bundled ELF | Rust toolchain and build container are pinned; Cargo lock is frozen; paths, locale, timezone and epoch are normalized. CI builds twice, compares bytes, compares the committed ELF, verifies SHA-256/version, and produces GitHub build provenance on trusted pushes. | `Reproducible runtime` workflow and GitHub attestation | Provenance proves which workflow built the bytes, not their safety. |

## Audit procedure

The release gate consists of Rust format and Clippy checks, all unit and isolated
lifecycle tests, two-build byte reproducibility, committed-binary equivalence,
the pinned Omarchy validator, project validation, and the native Quattro matrix
in `docs/live-acceptance.md`.

The authoritative limitations remain in `SECURITY.md` and must be read before
running untrusted project checks or plugins.

