# Reproducible x86-64 runtime

The bundled helper is built from `Cargo.lock` with Rust 1.98.0 inside the immutable
container digest recorded in `.github/workflows/reproducible-runtime.yml`.

CI builds the helper twice into independent target directories, requires the two
outputs to be byte-identical, and then requires that output to be byte-identical
to `bin/omarchy-plugin-workbench-x86_64`. `bin/SHA256SUMS` records the resulting
digest. A mismatch blocks the pull request.

On trusted pushes to `main` and version tags, GitHub's build-provenance action
attests the committed binary. The attestation binds its SHA-256 digest to the
repository, workflow and exact Git commit in GitHub's transparency log.

## Reproduce locally

Install Docker, check out the exact commit, then run:

```bash
export WORKBENCH_BUILD_IMAGE='IMAGE_WITH_IMMUTABLE_SHA256_DIGEST'
scripts/build-reproducible.sh /tmp/workbench-build-a
scripts/build-reproducible.sh /tmp/workbench-build-b
cmp /tmp/workbench-build-a/release/omarchy-plugin-workbench \
  /tmp/workbench-build-b/release/omarchy-plugin-workbench
cmp /tmp/workbench-build-a/release/omarchy-plugin-workbench \
  bin/omarchy-plugin-workbench-x86_64
sha256sum --check bin/SHA256SUMS
```

Use the exact digest from the workflow rather than a mutable container tag.

After a trusted workflow finishes, verify provenance with the GitHub CLI:

```bash
gh attestation verify bin/omarchy-plugin-workbench-x86_64 \
  --repo tcballard/omarchy-plugin-workbench
```

The attestation establishes build provenance, not that the program is safe.

