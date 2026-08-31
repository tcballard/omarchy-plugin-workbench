# Contributing

Create a focused branch and pull request. Do not commit directly to `main`.

Before opening a PR, run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
scripts/validate.sh
```

Filesystem or process-execution changes must include adversarial tests. QML changes must be validated with the pinned Omarchy contract and accepted on a real Quattro session before release.

Do not weaken unmanaged-target preservation, project-check trust, path canonicalisation, timeout, or output-boundary behavior to make a test pass.
