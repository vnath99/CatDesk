# T-0283 / T-0223-R4E-R3 protected absence atomic rollback

## Disposition

T-0281 left one deliberate fail-closed blocker: a clean bootstrap could not restore a prior absent protected receipt without generic deletion authority. This repair adds only the required fixed transaction capability. It preserves T-0281's by-value `VARIANT` ABI and semantic Task Scheduler XML protections unchanged.

**Conclusion: `READY_FOR_FRESH_T0274_HOST_ACTIVATION`, subject to independent ChatGPT source review.** This is not host-live acceptance and no activation was attempted.

## Fixed receipt presence snapshot

`SupervisorInstallReceiptSnapshotV1` records the two fixed protected children independently as either `Present { exact validated receipt, exact bytes }` or `Absent`. Absence is produced only by `read_optional_relative_regular` under the pinned supervisor root: it is a no-follow, RootDirectory-relative missing-child result, never `Path::exists`, metadata probing, or a pathname reopen.

| Prior current | Prior LKG | Commit result | Exact restore |
| --- | --- | --- | --- |
| Absent | Absent | current becomes prepared; LKG remains absent | remove only matching transaction-produced current |
| Present | Absent | current becomes prepared; LKG becomes prior current | remove matching transaction-produced LKG, restore current bytes |
| Absent | Present | current becomes prepared; LKG remains prior value | remove matching transaction-produced current |
| Present | Present | current becomes prepared; LKG becomes prior current | restore exact prior LKG and current bytes |

## Exact-child removal authority

The narrowly shared primitive `remove_exact_relative_regular_with_bytes` is crate-private and accepts only a pinned parent, bounded direct-child component, bounded expected bytes, and a diagnostic label. It opens the exact direct child no-follow with `RootDirectory`, rejects reparse/directory/special substitution, reads and compares the same opened handle's bytes, applies native `FileDispositionInformation` to that handle, closes it, then reopens only the same fixed direct child to prove absence. It provides no enumeration, caller-visible path opening, arbitrary root selection, or generic cleanup surface.

`SupervisorInstallerWriterV1` is the sole consumer for `supervisor-current.json` and `supervisor-lkg.json`. It revalidates allowed live transaction states before restore. Any foreign, concurrent, reparse, malformed, changed-byte, or post-disposition non-absence state is a rollback failure; lifecycle returns `SUPERVISOR_ACTIVATION_COMPENSATION_FAILED` and never reports readiness.

## Transaction and failure matrix

1. Read-only policy/image/state/task checks and task snapshot.
2. Protected current/LKG presence snapshot.
3. Inert reviewed side-by-side version preparation.
4. Exact fixed task staged disabled.
5. Protected current/LKG commit.
6. Exact fixed task final-enable and post-verification.

| Failure point | Required outcome |
| --- | --- |
| Scheduler stage | No receipt mutation |
| Current/LKG commit | Restore receipt presence/content, then revalidated prior task/absence |
| Final enable/update/postverify | Restore receipt presence/content first, then revalidated prior task/absence |
| Receipt or task compensation cannot be proven | Distinct compensation failure; no launch/readiness |

The present/absent combination test exercises all four receipt shapes, including exact removal for absent restoration. Existing protected-FS containment and receipt tests continue to reject unsafe reparse/type/identity outcomes. No mutation/deletion occurs outside the two fixed receipt children in isolated fixture roots.

## Verification

| Gate | Result |
| --- | --- |
| Focused `control_plane_supervisor` | PASS (28 tests) |
| Focused `supervisor_lifecycle` | PASS (10 tests) |
| Focused `windows_protected_fs` | PASS (5 tests) |
| T-0281 `windows_supervisor_startup` ABI/XML tests | Preserved; PASS in full suite |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (828 tests; existing opt-in ignored tests unchanged) |
| `cargo build --all-targets --all-features` | PASS |
| `cargo check --release --bin catdesk --bin catdesk-control-plane-supervisor` | PASS |
| Project `rust_full` | Not separately configured/discoverable; not inferred |
| `git diff --check` | PASS (pre-existing line-ending warnings only) |

## Attribution and boundaries

T-0283 changes are limited to `src/windows_protected_fs.rs`, `src/control_plane_supervisor.rs`, `src/supervisor_lifecycle.rs`, and this bundle. The repository was intentionally broadly dirty/untracked; no clean/reset/stage/commit/publication occurred and unrelated paths are not attributed.

No live Task Scheduler, ProgramData, port, pipe, daemon, browser/wake, Secure-MCP/tunnel, signing/provenance, dedicated-producer, or Git mutation was performed. No shell scheduler, SCM, LocalSystem, Run-key, Startup-folder, caller-selected authority, or supervisor launch was introduced.

Request independent ChatGPT review before any fresh T-0274 host-live activation. T-0274 and T-0282 were not started in this session.
