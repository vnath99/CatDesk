# T-0273 / T-0223-R4A — Protected Supervisor Install/State Activation Readiness

## Scope and inherited boundary

T-0271 is retained as the accepted principal-bound control-pipe repair: the
fixed `127.0.0.1:3201` front door, fixed pipe, exact `127.0.0.1:3200` worker
gate, supervisor-token-derived same-user/same-session admission before decode,
OS-observed image digest, reviewed-manifest binding, generation CAS,
idempotency, recovery-role refusal, and old-backend preservation are unchanged.

T-0273 changes only activation-critical ProgramData supervisor filesystem
authority. It does not install, launch, bind, schedule, reload, promote, or
mutate the live supervisor or its ProgramData root.

## Before/after authority map

| Operation | Before | T-0273 protected authority |
| --- | --- | --- |
| Fixed root / `CatDesk` / supervisor root creation and descent | pathname metadata and recursive directory creation | `PinnedDirectory::acquire` at the fixed `C:\\ProgramData` anchor, then retained `ProtectedDirectoryGuard` RootDirectory/no-follow component descent/create |
| State load | pathname read | `read_optional_relative_regular` from the pinned final guard; only exact NT missing-name is initial absence |
| State save / commit | pathname temporary write and rename | `write_new_regular_for_atomic_replace`: handle-bound write, `sync_all`, size check, parent revalidation, and same-parent `NtSetInformationFile` rename |
| Install writer lock | create-new pathname plus stale pathname delete | delete-on-close relative regular lock; pre-existing/stale/substituted lock fails closed and has no delete fallback |
| `versions`, version, staging, image | pathname create/open/read/write/hash | retained guard chain, direct relative create/open/read/write/hash; staging is an explicitly renameable direct child committed to its same pinned parent |
| Current/LKG receipts | pathname read/write/rename | bounded relative receipt reads and atomic protected replacement |
| Replay/crash recovery | pathname cleanup could be tempting | only durable current/LKG evidence is read; stale lock/staging recovery is deliberately refused rather than enumerated or deleted by pathname |

The shared primitives are in `src/windows_protected_fs.rs`; no duplicate NT or
Win32 trust model was added. The production fixed-root branch retains handles
for `C:\\ProgramData`, `CatDesk`, and `ControlPlaneSupervisor` throughout an
operation. The crate-private fixture seam applies the same parent-pinned
component rules to isolated temporary roots.

## No-follow and commit semantics

All migrated child opens use `OBJECT_ATTRIBUTES.RootDirectory` with
`FILE_OPEN_REPARSE_POINT`, validate directory/file type and reparse state, and
revalidate the complete guard chain around mutation boundaries. Optional state
reads distinguish only exact `STATUS_OBJECT_NAME_NOT_FOUND`; malformed,
special, reparse, access, drift, and generic I/O results are not absence.

The atomic receipt/state commit renames a flushed direct temporary child under
the same pinned parent. Windows changes the destination directory entry rather
than resolving a destination symlink; it never writes through a link target.
The lock is delete-on-close, so a crashed owner releases its lock while an
already-present stale/substituted name fails closed instead of gaining unsafe
cleanup authority.

## Read-only activation readiness

`assess_fixed_supervisor_activation_readiness()` is a fixed-path, read-only
assessment. It neither creates the root nor mutates state. It checks:

- compiled exact loopback/pipe policy (`127.0.0.1:3201`, pipe, and exact
  `127.0.0.1:3200/mcp`);
- pinned root and schema-1 supervisor state parsing;
- presence and bounded parsing of the current receipt;
- exact `versions/<first-16-digest>` receipt form and pinned image digest;
- the optional LKG receipt with the same pinned image check.

Its only results are `SUPERVISOR_ACTIVATION_READY`,
`SUPERVISOR_ROOT_UNAVAILABLE`, `SUPERVISOR_STATE_INVALID`,
`SUPERVISOR_CURRENT_RECEIPT_MISSING`,
`SUPERVISOR_RECEIPT_OR_IMAGE_INVALID`, and
`SUPERVISOR_FIXED_POLICY_INVALID`.

## Adversarial and regression evidence

| Case | Evidence/result |
| --- | --- |
| First-run missing state | exact NT missing result is accepted as empty state; all other failures refuse |
| Root file replacement / oversized state | existing `state_root_rejects_file_replacement_and_oversized_state` remains passing |
| Parent/child identity chain and replacement resistance | existing protected guard clone/replacement/rename tests remain passing |
| Same-parent staging commit / outside sentinel | shared `protected_directory_guard_rename_is_pinned_same_parent_and_fail_closed` remains passing and preserves outside sentinel |
| Current/image digest or malformed receipt drift | new readiness fixture returns `SUPERVISOR_RECEIPT_OR_IMAGE_INVALID` |
| Absent root | new readiness fixture returns `SUPERVISOR_ROOT_UNAVAILABLE` and proves the path is not created |
| Read-only assessment | new fixture compares current receipt and image bytes before/after a successful assessment |
| Reintroduced pathname authority | scoped source regression scans only migrated store/installer authority bodies for `Path::exists`, `OpenOptions`, `fs::read/write/rename/remove_file/create_dir_all` |

The residual whole-file scan finds `fs::read` only in the separately scoped
reviewed current-worker-image evidence path and isolated test fixtures; the
migrated authority regression covers the store/install bodies specifically.

## Attributable files

- `src/control_plane_supervisor.rs` — protected root/store/installer migration,
  fail-closed stale lock behavior, read-only readiness API, and focused source/
  fixture regressions.
- `src/windows_protected_fs.rs` — minimal shared optional exact-missing read,
  delete-on-close lock, and atomic direct-child write/flush/rename primitives.
- `src/bin/catdesk-control-plane-supervisor.rs` — shared protected-FS module is
  included by the standalone zero-argument supervisor compilation unit.
- This bundle.

The repository was broadly dirty before T-0273; no cleanup/reset or unrelated
attribution was performed.

## Verification

| Command | Result |
| --- | --- |
| `cargo test control_plane_supervisor --no-fail-fast` | PASS (22 focused tests in each applicable binary context) |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features` | PASS (799 tests) |
| `cargo build --release` | Timed out at the fixed 120-second command ceiling; no pass is claimed |
| project `rust_full` | No separately runnable repository command discovered; not fabricated as a pass |
| `git diff --check` | PASS |
| scoped forbidden-authority scan | PASS for migrated production bodies; whole-file residuals are the separately scoped worker-image path and tests described above |

## Prohibited live actions

Not performed: real `.catdesk` mutation, ProgramData root mutation, supervisor
or pipe/3201 binding, worker/daemon/release/Scheduler/service changes, browser
or Secure-MCP/tunnel work, signing/provenance work, branch changes, or Git
publication.

## Conclusion

**READY_FOR_T0274_HOST_ACTIVATION** for independent source review only. ChatGPT
must independently inspect this bundle/source/diff and then perform any
separately authorized host-live continuity activation. Controller/source green
alone is not T-0223 closure.
