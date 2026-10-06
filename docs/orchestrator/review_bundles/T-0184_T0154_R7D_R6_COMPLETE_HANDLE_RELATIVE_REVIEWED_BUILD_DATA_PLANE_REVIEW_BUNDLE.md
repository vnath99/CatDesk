# T-0184 / T-0154-R7D-R6 — Reviewed-build data-plane audit

## Reproduced R5 rejection

The product audit of `src/reviewed_build.rs` found that R5 converted control
create/read children only.  Remaining path-oriented authority candidates are:

- claim replacement through `atomic_json`;
- snapshot materialization and nested destination creation;
- `target/release/catdesk.exe` acquisition;
- candidate creation, copy, hash, and replay validation;
- promotion-control attestation mirror write;
- generic path `read_regular_file`, `copy_regular_file_create_new`,
  `safe_workspace_regular_file`, `safe_workspace_output_file`, and
  `atomic_json` helpers.

The accepted R7C RootDirectory/no-follow primitives are the required mechanism
for every one of these children.  A pinned parent is insufficient when a child
is subsequently reopened by pathname.  This inventory is deliberately
fail-closed evidence: no live reviewed-build/promotion/reload action was
performed while these boundaries were under repair.

## Existing shared chain

Reviewed-build control state already holds the R7C `ProtectedDirectoryGuard`
for workspace → `.catdesk` → `reviewed-build-control`; attempt/claim/owner/
result/attestation create/read now use the shared relative regular-file helpers.
R6 additionally closes the control child namespace in production:
`CONTROL_CHILDREN` is the only set accepted by the handle-relative
`control_create_json`/`control_read_json` boundary; arbitrary child names,
path separators, and promotion names are rejected before a child open.
R4 owner proof, pinned tool identity, suspended Job Object launch, R7C snapshot
authority, and R6/R6A/R6B promotion consumption are unchanged.

## Required completion direction

The remaining data-plane must be moved to the same shared child object API:
open source/built/candidate/mirror objects only relative to pinned parent
handles; hash/copy through those opened handles; use handle-bound rename,
enumeration, and disposition; and fail closed on replacement/reparse/identity
drift.  No pathname helper may carry authority after parent classification.

## Verification

This ticket performed a bounded source audit only and did not invoke a live
worker, promotion, reload, recovery, tunnel, browser, Scheduler, external
project, or Git publication.  CatDesk independently determines completion.
