# T-0281 / T-0223-R4E-R2 native startup exact ABI and rollback completion

## Disposition

T-0280 was controller-verified but independently rejected. This source-only repair corrects the remaining Task Scheduler ABI mismatch, rejects XML normalization ambiguity, and adds a fixed-purpose protected current/LKG snapshot restore path for the supported receipt-present transaction shape. No live Task Scheduler task, ProgramData root, port, pipe, daemon, browser, wake, Secure-MCP/tunnel, or Git state was mutated.

**Conclusion: `PROTECTED_RECEIPT_ABSENCE_RESTORE_UNAVAILABLE`.** Receipt-present upgrade/reconcile transactions are rollback-safe in source, but a clean first-install shape with absent `current` or LKG remains intentionally non-activatable: the accepted protected-filesystem API has no bounded exact-file removal primitive, and this ticket did not introduce generic deletion authority. Independent ChatGPT review is required before any further repair or T-0274 work.

## Official SDK ABI to Rust matrix

| COM interface | Taskschd SDK method | inherited slot | Rust `extern "system"` parameters |
| --- | --- | ---: | --- |
| `IUnknown` | `Release` | 2 | `this -> ULONG` |
| `ITaskService` | `GetFolder` | 7 | `this, BSTR, ITaskFolder**` |
| `ITaskService` | `Connect` | 10 | `this, VARIANT, VARIANT, VARIANT, VARIANT` **by value** |
| `ITaskFolder` | `GetTask` | 13 | `this, BSTR, IRegisteredTask**` |
| `ITaskFolder` | `DeleteTask` | 15 | `this, BSTR, flags` |
| `ITaskFolder` | `RegisterTask` | 16 | `this, BSTR, BSTR, flags, VARIANT, VARIANT, logon type, VARIANT, IRegisteredTask**`; all three `VARIANT`s **by value** |
| `IRegisteredTask` | `get_Xml` | 20 | `this, BSTR*` |

The crate-local `#[repr(C)]` `VARIANT` has the 8-byte VARTYPE/reserved header and an 8-byte union, giving 16-byte size and at least 8-byte alignment on supported Windows targets. Native tests type-check exact by-value function-pointer assignments, assert union/`VARIANT` layout, and retain fake-vtable HRESULT/slot dispatch evidence. No `*const Variant` aliases remain in these SDK method signatures.

## Semantic exact-owned task decision

The bounded XML parser now fails closed instead of deduplicating normalized attributes. Exact ownership requires the fixed URI, one expected SID principal with `InteractiveToken` and `LeastPrivilege`, exactly one logon trigger for that SID, one `Exec` with the planned reviewed image path and exact supervisor mode argument, no working directory or other action, and the fixed settings set.

| XML condition | Classification |
| --- | --- |
| Exact fixed definition | `ExactOwned` |
| Duplicate local/semantic attribute | Foreign/ambiguous |
| Namespace declaration/rebinding or prefixed semantic name | Foreign/ambiguous |
| Extra principal, trigger, action, Exec, Arguments, WorkingDirectory, or setting | Foreign/ambiguous |
| Time/Boot/Event trigger, `ComHandler`, wrong SID, elevated run level, malformed XML | Foreign/ambiguous |

Whitespace and attribute order remain harmless where they do not change the fixed semantic tree; a namespace prefix is deliberately refused because this local parser does not retain namespace scope well enough to prove that it cannot be rebound.

## Protected receipt/task transaction

For the supported current+LKG-present shape, activation is ordered as follows:

1. Read-only principal/runtime/image/state/task checks and exact task classification.
2. Under the pinned protected root, snapshot both independently validated current and LKG receipts.
3. Prepare the reviewed version side-by-side; no current/LKG advance.
4. Stage only the exact scheduler task disabled.
5. Commit protected current/LKG.
6. Final-enable and re-verify the exact task.

| Failure boundary | Current/LKG | Task | Result |
| --- | --- | --- | --- |
| Before or during task stage | Unchanged | Unchanged | Fail closed |
| Current commit after disabled stage | Snapshot restore then narrow exact task compensation | Prior task/absence restored only after live revalidation | Fail closed |
| Final enable/update/readback after commit | Exact current+LKG snapshot restore first, then task restore | Prior exact task/absence restored | Fail closed |
| Any receipt/task compensation mismatch or failure | Not asserted ready | No broad overwrite/delete | `SUPERVISOR_ACTIVATION_COMPENSATION_FAILED` |
| Prior current or LKG absent | No mutation begins | Unchanged | `PROTECTED_RECEIPT_ABSENCE_RESTORE_UNAVAILABLE` boundary |

LKG is separately snapshotted and restored; it is recovery material, not an implicit transaction snapshot. Restore accepts only the known pre-transaction or transaction-produced receipt pair under the same pinned root. It is not a caller/path/generic recovery API.

## Evidence and verification

| Gate | Result |
| --- | --- |
| Focused `windows_supervisor_startup` | PASS (10 tests) |
| Protected receipt snapshot/restore test | PASS |
| Focused lifecycle/control-plane suite | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (827 tests; existing opt-in tests remain ignored) |
| `cargo build --all-targets --all-features` | PASS |
| `cargo check --release --bin catdesk --bin catdesk-control-plane-supervisor` | PASS |
| Project `rust_full` | Not separately configured/discoverable; not inferred |
| `git diff --check` | PASS (pre-existing line-ending warnings only) |

## Attribution and boundaries

T-0281-attributable paths are `src/windows_supervisor_startup.rs`, `src/supervisor_lifecycle.rs`, `src/control_plane_supervisor.rs`, and this bundle. The worktree was intentionally dirty/untracked before this task; no unrelated path is attributed, and no clean/reset/stage/commit/publication occurred.

No PowerShell, `schtasks`, cmd, generic process scheduler authority, LocalSystem/SCM/Run-key/Startup-folder persistence, caller-selected startup fields, target/release/PATH/CWD/current-executable trust, signing/provenance work, Secure-MCP/tunnel ownership, or live host mutation was introduced.

Please perform independent ChatGPT review. Do not start T-0274 host-live activation from this ticket.
