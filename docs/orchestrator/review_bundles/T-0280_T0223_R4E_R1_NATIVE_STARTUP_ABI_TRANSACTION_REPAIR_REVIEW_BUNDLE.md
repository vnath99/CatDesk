# T-0280 / T-0223-R4E-R1 native startup ABI and transaction repair

## Disposition

T-0279 was controller-verified but independently rejected. This repair addresses its three concrete blockers: inherited Task Scheduler COM ABI slots, semantic task ownership, and clean-install/cross-resource commit ordering. This is source/test evidence only: no Task Scheduler task, ProgramData state, listener, pipe, daemon, browser, wake, Secure-MCP/tunnel, or Git state was mutated.

**Conclusion: READY_FOR_FRESH_T0274_HOST_ACTIVATION, subject to independent ChatGPT review.** Controller/test green alone is not host-live acceptance.

## Corrected COM ABI matrix

Raw COM remains crate-local to avoid a broad new binding/dependency surface. Each call now uses an interface-specific alias and named inherited SDK slot rather than a generic literal.

| Interface | Method | SDK slot | Signature used |
| --- | --- | --- |
| `IUnknown` | `Release` | 2 | `fn(this) -> ULONG` |
| `ITaskService` | `GetFolder` | 7 | `fn(this, BSTR, **ITaskFolder) -> HRESULT` |
| `ITaskService` | `Connect` | 10 | `fn(this, VARIANT, VARIANT, VARIANT, VARIANT) -> HRESULT` |
| `ITaskFolder` | `GetTask` | 13 | `fn(this, BSTR, **IRegisteredTask) -> HRESULT` |
| `ITaskFolder` | `DeleteTask` | 15 | `fn(this, BSTR, flags) -> HRESULT` |
| `ITaskFolder` | `RegisterTask` | 16 | `fn(this, BSTR, BSTR, flags, VARIANT, VARIANT, TASK_LOGON_TYPE, VARIANT, **IRegisteredTask) -> HRESULT` |
| `IRegisteredTask` | `get_Xml` | 20 | `fn(this, *BSTR) -> HRESULT` |

`ITaskService` includes `IUnknown` slots 0–2 and `IDispatch` 3–6, so `GetFolder=7`, `GetRunningTasks=8`, `NewTask=9`, and `Connect=10`. T-0279’s `Connect=7` dispatch was removed. COM apartment, BSTR, VARIANT, HRESULT, and Release ownership remains explicit. Fake-vtable tests assert each slot, dispatch order, HRESULT preservation, and typed ABI aliases.

## Semantic exact-owned classifier

The former substring/count classifier is replaced by a bounded parsed Scheduler XML tree. Namespace prefixes and attribute ordering are tolerated; malformed XML, declarations/DTD/comments, duplicate/unknown nodes, unknown entities, and all widened task semantics fail closed.

Exact ownership requires all of the following:

| Area | Exact rule |
| --- | --- |
| Task identity | One `Task` with only RegistrationInfo, Triggers, Principals, Settings, Actions and URI `\CatDeskStableSupervisorV1` |
| Principal | Exactly one principal, exact TokenUser SID, `InteractiveToken`, `LeastPrivilege` |
| Trigger | Exactly one enabled LogonTrigger for that same SID; no time/boot/event/second-logon trigger |
| Action | Exactly one `Exec`, exact reviewed planned path, exact `--catdesk-control-plane-supervisor`, no working directory or alternate action |
| Settings | Only explicit fixed `Enabled`, `IgnoreNew`, `StartWhenAvailable=false`, `ExecutionTimeLimit=PT0S` semantics |

Adversarial coverage rejects Time/Boot/Event triggers, second LogonTrigger, second/wrong Exec, ComHandler, wrong principal/SID, elevated run level, changed settings, added working directory, extra args, malformed XML, duplicate nodes, and foreign tasks.

## Planned image and cross-resource transaction

`planned_reviewed_supervisor_startup_action_path(digest)` derives the future fixed version action solely from the T-0278 internal reviewed-image digest and fixed installer root. It does not require a `current` receipt, making clean protected state plus Scheduler Absent activatable without `target/release`, PATH, CWD, sibling/current-executable, caller image, hash, or worker-digest authority.

`SupervisorInstallerWriterV1` now separates:

1. `prepare_exact_reviewed_image` — validates reviewed bytes/digest and creates/verifies only the protected side-by-side version; it does not advance current/LKG.
2. Scheduler stage — writes/updates an exact task **disabled**. It cannot run an inert prepared image.
3. `commit_prepared_exact_reviewed_image` — freshly opens/hashes prepared version, then advances protected current/LKG receipt.
4. Final task update — re-reads exact disabled task and enables the exact fixed definition.

Scheduler registration failure therefore leaves current unchanged. If current commit fails after disabled staging, narrow compensation re-reads the exact live staged task: an Absent predecessor permits deletion only of the transaction’s exact task; an ExactOwned predecessor permits restoration only from that exact staged task to the captured exact predecessor. Changed/foreign/ambiguous task state is never deleted or overwritten. LKG is explicitly recovery material, not transaction rollback.

| Failure point | Current receipt | Scheduler task | Result |
| --- | --- | --- | --- |
| Prepare fails | Unchanged | Unchanged | Fail closed |
| Disabled scheduler stage fails | Unchanged | Unchanged/foreign untouched | Fail closed |
| Current commit fails | Unchanged | Narrow re-read compensation only | Fail closed |
| Enable/final readback fails | Newly committed | Disabled/non-runnable exact task remains | Fail closed; retry can reconcile without broad cleanup |

## Verification

| Gate | Result |
| --- | --- |
| Focused `windows_supervisor_startup` | PASS (8 tests, including fake ABI dispatch and semantic adversaries) |
| Focused `supervisor_lifecycle` | PASS (10 tests) |
| Focused `control_plane_supervisor` | PASS (26 tests, including inert prepare/explicit commit) |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (824 tests; existing opt-in Python tests remain ignored) |
| `cargo build --all-targets --all-features` | PASS |
| `cargo check --release --bin catdesk --bin catdesk-control-plane-supervisor` | PASS |
| Project `rust_full` harness | Not separately configured/discoverable; not inferred |
| `git diff --check` | PASS (pre-existing line-ending warnings only) |

## Attribution and boundaries

T-0280 changes are limited to `src/windows_supervisor_startup.rs`, `src/supervisor_lifecycle.rs`, `src/control_plane_supervisor.rs`, and this bundle. The repository was intentionally broadly dirty/untracked before work; no clean/reset/stage/commit/publication occurred and unrelated material is not attributed.

No PowerShell, scheduler executable, cmd, generic process command, SCM/LocalSystem/Run-key/Startup-folder persistence, caller-selected startup authority, signing/provenance/dedicated-producer work, Secure-MCP/tunnel ownership, or live host mutation was added.

Please perform independent ChatGPT review before any fresh T-0274 host-live activation or later work.
