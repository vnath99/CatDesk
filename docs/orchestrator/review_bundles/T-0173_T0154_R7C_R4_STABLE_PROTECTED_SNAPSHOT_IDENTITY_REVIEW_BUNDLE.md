# T-0173 / T-0154-R7C-R4 stable protected snapshot identity

## T-0172 rejection

R7C-R3 classified each protected path component before path-based creation and
mutation. A hostile rename or reparse replacement between that classification
and `create_dir`, byte write, rename, or cleanup could redirect authority
outside the workspace.

## Windows identity guard

The snapshot module now uses a `ProtectedDirectoryGuard`. On Windows it opens
the workspace and each protected component with `CreateFileW`,
`FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS`, and no
`FILE_SHARE_DELETE`. `GetFileInformationByHandle` binds the volume serial and
file index to the live handle. Every mutation boundary rechecks those handles;
rename/replacement or reparse identity drift fails closed. Non-Windows builds
fail closed when this stable identity primitive is unavailable.

Missing `.catdesk` and `reviewed-source-snapshots` components are created only
under a pinned parent with one `create_dir`, then immediately reopened and
pinned before use as another parent. The v4 manifest schema and all existing
authority/manifest/snapshot digest formulas remain unchanged.

## Mutation and cleanup

Staging creation is under the pinned snapshot root. Manifest and byte objects
use create-new, sync, and post-write regular-file classification, preventing a
pre-existing link or target overwrite. Final rename first verifies the pinned
root identity; a raced final result is accepted only through exact committed
snapshot validation. Owned cleanup retains the exact staging-name restriction
and refuses unsafe tree entries rather than recursing through them.

## Verification

Focused shared-module regressions passed: 10 passed, 0 failed. They retain
link-redirection, protected-root, stale-staging, byte replay, manifest/content
tamper, output-drift, include-asset, dynamic-include, and legacy-state checks.
No live build worker, promotion, reload, recovery, tunnel, browser, Scheduler,
external-project mutation, or Git publication was performed. T-0169/R7D
remains blocked pending independent host acceptance of this stable snapshot
identity layer.

Full verification passed: `cargo fmt --check`, clippy with warnings denied,
`cargo test` (600 passed, 18 ignored, 0 failed), both project PowerShell
fixtures, and `cargo build --release`.
