# Reproducible runtime

The helper is built from `Cargo.lock` with Rust 1.98.0 inside the immutable
container digest recorded in `.github/workflows/reproducible-runtime.yml`.

CI builds the helper twice into independent target directories, requires the two
outputs to be byte-identical, and publishes the candidate plus its build identity
as a workflow artifact. A mismatch blocks the pull request.

On version tags, GitHub's build-provenance action attests the runtime candidate.
The attestation binds its SHA-256 digest to the
repository, workflow and exact Git commit in GitHub's transparency log.

## Reproduce locally

Install Docker, check out the exact commit, then run:

```bash
export WORKBENCH_BUILD_IMAGE='rust@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922'
scripts/build-reproducible.sh /tmp/workbench-build-a
scripts/build-reproducible.sh /tmp/workbench-build-b
cmp /tmp/workbench-build-a/release/omarchy-plugin-workbench \
  /tmp/workbench-build-b/release/omarchy-plugin-workbench
sha256sum /tmp/workbench-build-a/release/omarchy-plugin-workbench
```

Use the exact digest from the workflow rather than a mutable container tag.

After a trusted workflow finishes, verify provenance with the GitHub CLI:

```bash
gh attestation verify /path/to/omarchy-plugin-workbench \
  --repo tcballard/omarchy-plugin-workbench
```

The attestation establishes build provenance, not that the program is safe.
