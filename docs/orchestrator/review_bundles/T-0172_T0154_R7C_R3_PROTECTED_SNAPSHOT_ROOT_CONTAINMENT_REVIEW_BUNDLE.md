# T-0172 / T-0154-R7C-R3 protected snapshot-root containment

## T-0171 rejection reproduced

R7C-R2 classified the final snapshot directory but could call `create_dir_all`
through `.catdesk` before proving that intermediate directory. A symlink or
Windows reparse point at `.catdesk` or `reviewed-source-snapshots` could
therefore redirect snapshot creation or validation outside the approved
workspace.

## Protected-chain algorithm

`reviewed_source_snapshot.rs` now starts at a positively classified workspace
directory and descends fixed components one at a time. Every existing component
is checked with `symlink_metadata`; symlinks, Windows reparse points, special
types, and metadata ambiguity fail closed. For a missing component, it calls
only `create_dir` after the immediate parent is already proven, then
immediately re-reads and classifies the newly created directory. It never uses
`create_dir_all` for the protected `.catdesk/reviewed-source-snapshots` chain.

The same chain is proven before final-session reads, manifest and byte-root
validation, staging creation, and commit. The accepted R7C-R2 v4 schema,
authority/manifest/snapshot digest calculations, attributed-output binding,
source inventory, and bounds are unchanged.

## Staging, replay, and races

Staging is a UUID-owned immediate child of the proven root. On an owned
pre-commit failure cleanup recursively removes only regular files and proven
directories; it refuses a link/reparse or an unexpected staging identity.
Staging is never authority. A stale staging directory is ignored when the
committed final snapshot fully validates. A staging-only crash state cannot
validate. Before rename the root is reclassified; rename races resolve only by
validating the exact final directory against expected authority, never by
recapturing mutable workspace bytes.

## Focused regression matrix

The shared module now tests safe single-component root creation, a file at an
intermediate protected component, `.catdesk` link redirection with no outside
mutation (when the host permits link creation), stale staging alongside a
valid committed replay, plus the retained R7C-R2 tests for binary nested bytes,
source drift replay, manifest tamper, output-evidence drift, literal include
assets, dynamic include rejection, and extra/legacy state rejection.

## Changed files and verification

- `src/reviewed_source_snapshot.rs`
- `docs/orchestrator/review_bundles/T-0172_T0154_R7C_R3_PROTECTED_SNAPSHOT_ROOT_CONTAINMENT_REVIEW_BUNDLE.md`

Focused snapshot tests passed: 10 passed, 0 failed. No live build worker,
promotion, daemon reload, recovery/fault injection, tunnel, browser, Scheduler,
external-project mutation, or Git publication occurred. T-0169/R7D remains
blocked pending independent acceptance of the snapshot authority layer.

Full verification also passed: `cargo fmt --check`, clippy with warnings
denied, `cargo test` (599 passed, 18 ignored, 0 failed), both project
PowerShell fixtures, and `cargo build --release`.
