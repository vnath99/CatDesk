# T-0407 R7 GitHub Publication Provenance Closure

## Scope and classification

`READY_FOR_REVIEWED_BUILD` — subject to independent CatDesk verification.

This bounded review changes the closed CatDesk-owned
PREPARE/CONFIRM/RESULT publication executor and its provenance bindings. No
commit, push, network publication, runtime, Wake, tunnel, recovery, or
protected-state operation was invoked.

## R7 closure

- The executor now obtains Git only through the reviewed fixed-candidate
  resolver shared with `github_bootstrap`; it never resolves `git` from PATH.
  PREPARE records canonical executable identity, selected candidate slot, and
  SHA-256 fingerprint. CONFIRM, push, and RESULT independently resolve and
  require the exact same identity before invoking a fixed argv.
- PREPARE allows an already staged *subset* only when every staged path is in
  the approved literal publication set. Any unrelated staged path fails
  closed. CONFIRM re-probes provenance, stages every literal approved path,
  then requires the staged set to equal the complete approved set exactly.
- The manifest parser retains each include path, SHA-256, and byte length.
  PREPARE and CONFIRM re-read every included file and refuse length or digest
  drift before staging or commit.
- Activation no longer depends on a compile-time manifest hash. The exact
  current reviewed output authority must contain precisely one R7 descriptor;
  that descriptor binds the manifest identity and the fixed supplemental
  descriptor/review artifacts. MCP remains closed: PREPARE accepts only a
  record ID and typed approval; CONFIRM and RESULT accept only a confirmation
  token.

## Frozen artifacts

- Manifest: `docs/orchestrator/T-0407_R7_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`
- Manifest SHA-256:
  `d7c78e3500c167287c463df9a8df2c01454b91de25ae9c7970ae79a65eadf753`
- Descriptor:
  `docs/orchestrator/T-0407_R7_GITHUB_PUBLICATION_DESCRIPTOR.json`
- Classifications: 937 `INCLUDE_RECONSTRUCTION`, 12
  `EXCLUDE_ARCHIVAL_BINARY`, and 1 `EXCLUDE_RUNTIME_GENERATED`.
- A direct re-read of all 937 included files found zero byte-length or
  SHA-256 mismatches; the descriptor manifest digest matched exactly.

The manifest includes current R5/R6/R7 publication source and tests. The R7
manifest, descriptor, and this review bundle are supplemental fixed artifacts
outside the manifest hash set, preventing a self-reference cycle.

## Deterministic coverage

Focused `github_publication` tests passed (17): trusted-Git fingerprint drift,
approved pre-staged subset, unrelated staged rejection, one-byte manifest
drift, descriptor mismatch, fixed literal staging/commit, no-force push argv,
exact remote branch matching, redacted bounded token, journal replay/recovery,
and closed MCP schema coverage.

## Verification

- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `cargo test --workspace --all-targets --all-features`: passed (1,010 tests;
  existing explicitly ignored tests remained ignored).
- `cargo fmt --all -- --check`: passed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File
  scripts/test-catdesk-lifecycle.ps1`: passed (`consumer lifecycle fixture
  tests passed`).
- `git diff --check`: passed.

## Residual activation boundary

The executor is intentionally inert until a later reviewed source snapshot
contains the single exact descriptor output and an authorized typed GitPush
approval is supplied. Any Git executable, repository, branch, local HEAD,
manifest, per-file byte/hash, staged-set, review-output, confirmation-token,
or remote-ref drift fails closed. Independent review remains required before
activation.
