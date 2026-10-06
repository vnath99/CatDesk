# T-0279 / T-0223-R4E native same-principal supervisor startup authority

## Disposition

This source/test slice adds the remaining closed native Windows startup authority after the independently accepted T-0278 reviewed supervisor-image role binding. It does not perform host activation, create a Task Scheduler task, mutate ProgramData, bind 3201, start the supervisor, restart the worker, or alter Secure-MCP/tunnel, browser, wake, Git, signing, or provenance state.

**Status: READY FOR INDEPENDENT CHATGPT REVIEW.** A fresh bounded T-0274 host-live activation/continuity review remains required.

## Authority chain and fixed definition

`windows_supervisor_startup::fixed_supervisor_startup_authority()` is the sole production constructor for the opaque `SupervisorStartupAuthorityV1`. It reuses `windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()`, which in turn reuses the production fixed-pipe current-token derivation:

1. Open the current process token through the Windows API.
2. Derive TokenUser and TokenSessionId from OS evidence.
3. Reject LocalSystem (`S-1-5-18`) and session 0.
4. Retain only the SID in the opaque startup authority.

The numeric session ID is an admission proof that the current caller is interactive. It is deliberately not serialized into the future-logon startup definition; the task binds a user SID and interactive-token logon behavior instead.

| Field | Fixed value / rule |
| --- | --- |
| Native API | Task Scheduler COM (`ITaskService`, root `ITaskFolder`, `IRegisteredTask`) through crate-local FFI |
| Task identity | `CatDeskStableSupervisorV1` at `\CatDeskStableSupervisorV1` |
| Trigger | One enabled `LogonTrigger` for the OS-derived TokenUser SID |
| Logon type | `TASK_LOGON_INTERACTIVE_TOKEN` / `InteractiveToken` |
| Run level | `LeastPrivilege` |
| Credentials | No password, credential, environment secret, or numeric session persisted |
| Action | Only receipt-bound protected installer image path |
| Arguments | Exactly `--catdesk-control-plane-supervisor` |
| Working directory / restart policy / ports / pipe | Not selectable and not present in the public/operator grammar |

The action path comes only from `fixed_current_supervisor_startup_action_path()`. That internal bridge opens the fixed protected root, reads and validates the current receipt, validates the exact opened version image digest under the pinned/no-follow guard, rechecks root identity, and only then supplies the Task Scheduler-required action string. It does not accept an operator path or reopen that string as filesystem authority.

## Existing definition policy

The read-only classifier is `Absent`, `ExactOwned`, or `ForeignOrAmbiguous`.

| Existing task at fixed name | Activation behavior |
| --- | --- |
| Absent | May create exactly the fixed definition with `TASK_CREATE`, then read back and require `ExactOwned` |
| Exact owned definition | Idempotently accepted |
| Exact prior owned definition after image version change | Re-read exact prior XML, then narrowly update only that proven owned definition and post-verify the new exact XML |
| Foreign, malformed, mismatched, concurrent replacement, or inaccessible definition | Fail closed; never delete or blindly overwrite |

The XML classifier requires the fixed URI, two exact SID occurrences, `InteractiveToken`, `LeastPrivilege`, one fixed `Exec` command, one fixed argument, and rejects password, elevated run level, or COM-handler alternatives. The native code uses `TASK_CREATE` for absent state; it has no combined create-or-overwrite path.

## Lifecycle transaction

`supervisor_lifecycle::activate_fixed_authority` now records all scheduler preconditions before its first installer write:

1. Preserve T-0277 typed fixed-pipe principal and stable-runtime policy results.
2. Require the distinct T-0278 reviewed supervisor-image role capability.
3. Require opaque native startup authority and receipt-bound current action.
4. Classify the fixed task and reject foreign/ambiguous state.
5. Validate existing protected state before calling `install_exact_reviewed_image`.
6. Install only bytes/digest from the reviewed role capability using the existing protected Writer/LKG protocol.
7. Derive the post-install receipt-bound action, create or narrowly update the task, and read it back.
8. If scheduler finalization fails, return `SUPERVISOR_STARTUP_REGISTRATION_FAILED`; the supervisor is not launched and the Writer's accepted LKG preservation remains available for reviewed recovery.

Status and preflight are read-only. Status reports only fixed non-secret categories including startup authority and startup policy. Preflight distinguishes `SUPERVISOR_STARTUP_AUTHORITY_UNAVAILABLE`, `ELEVATION_REQUIRED`, `SUPERVISOR_STARTUP_POLICY_UNPROVEN`, `SUPERVISOR_STARTUP_REGISTRATION_FAILED`, protected-state/image failures, and the preserved typed principal/runtime failures.

## No-caller-authority and non-ownership matrix

| Authority | Result |
| --- | --- |
| SID, session, user, task, action path, arguments | Derived internally or compiled; no caller constructor or operator grammar |
| Startup persistence | Native Task Scheduler COM only; no PowerShell, shell scheduler executable, `cmd`, generic process command, Run key, Startup folder, SCM service, LocalSystem, or stored password |
| Supervisor image | T-0278 explicit role capability only; never worker digest bridge, build output, CWD, PATH, sibling, filename, or caller bytes/hash |
| Protected image/state | Existing fixed Writer/current/LKG pinned no-follow authority retained |
| Worker lifecycle | Not started, stopped, restarted, or owned here |
| Secure-MCP/tunnel | Not started, stopped, configured, or owned here |
| Browser/wake | Not inspected or invoked |

## Deterministic coverage

- Opaque test seam accepts only non-system SID shape; empty, LocalSystem, malformed, and non-SID values fail.
- Exact generated definition includes exactly the interactive trigger, least privilege, fixed URI/action/argument and excludes password, session persistence, LocalSystem, elevated run level, and COM handler alternatives.
- Altered logon type, run level, or argument is classified as non-exact/foreign.
- Source guard rejects shell/service/Run-key/Startup-folder/current-executable/release shortcut authority in the new startup module and rejects a combined create-or-overwrite registration mode.
- Lifecycle source regression proves scheduler classification precedes installer mutation and post-install registration follows it.
- Existing lifecycle regressions retain exact grammar, typed principal/runtime failures, invalid-state-before-installer refusal, reviewed-image separation, idempotent protected installer behavior, and LKG retention.

The focused tests use isolated in-process XML/authority seams only. They do not import Selenium, register a task, create ProgramData state, bind a port/pipe, or launch a process.

## Verification

| Gate | Result |
| --- | --- |
| Focused `supervisor_lifecycle` | PASS (10 tests) |
| Focused `windows_supervisor_startup` | PASS (5 tests) |
| Focused `control_plane_supervisor` | PASS (25 tests) |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (820 tests; pre-existing Python-dependent tests remain ignored) |
| `cargo build --all-targets --all-features` | PASS |
| `cargo check --release --bin catdesk-control-plane-supervisor` | PASS |
| `cargo check --release --bin catdesk` | PASS |
| Project `rust_full` / verification harness | No separate configured/discoverable harness in this workspace; not inferred |
| Scoped authority scan and `git diff --check` | PASS (line-ending warnings only) |

Cargo emitted its existing `could not canonicalize path <USER_PROFILE>

## Attributable surface

- `src/windows_supervisor_startup.rs` — opaque same-user interactive authority, native Task Scheduler COM classifier/create/update/postverify path, and isolated regressions.
- `src/windows_supervisor_control_pipe.rs` — narrow SID-only bridge from the already accepted current-token principal derivation.
- `src/control_plane_supervisor.rs` — receipt-bound protected current-action bridge for the scheduler action.
- `src/supervisor_lifecycle.rs` — native authority/preflight/status/transaction composition and source ordering regression.
- `src/main.rs` — module declaration only.
- This bundle.

The worktree is intentionally broadly dirty and these sources were inherited as untracked prior task surfaces; whole-worktree Git diff is not used as attribution. No clean, reset, stage, commit, branch, publication, or host mutation was performed.

## Residual prerequisite

After independent source review, the remaining bounded work is a fresh T-0274/T-0223 host-live activation and continuity acceptance using only the reviewed operator lifecycle surface. It must independently preflight the reviewed image, protected current/LKG state, same-principal task definition, fixed pipe/ports, generation handling, and rollback behavior. This source ticket does not claim that host-live acceptance.
