# T-0154-R1 — one-command recovery convergence

## Scope and diagnosis

This source/test review reconciles the recorded T-0140 lifecycle closure, the
T-0121 fixed-vocabulary status boundary, and the T-0154 reviewed recovery
evidence against the current T-0223 stable-supervisor lifecycle.  It performs
no host activation, ProgramData mutation, Task Scheduler mutation, daemon
restart, browser wake, tunnel operation, target change, or Git action.

The closed stable lifecycle was already correctly limited to:

| Operator form | Authority | Mutation |
| --- | --- | --- |
| `operator supervisor status` | Fixed protected-state readiness classification | None |
| `operator supervisor preflight` | Fixed principal/runtime/image/startup readiness | None |
| `operator supervisor activate` | Fixed protected installer/current-LKG transaction and fixed startup policy | Only after all fixed checks |

`recover` is intentionally not an operator-supervisor verb.  The concrete
remaining defect was elsewhere: `daemon_reload::schedule_canonical_recovery`
could spawn a detached helper, and that helper invoked the historical
`catdesk.ps1 recover` PowerShell route.  That was a version-coupled recovery
escape hatch outside the reviewed stable lifecycle and could reach daemon and
external-runtime work after a delayed dispatch.

## Correction

`src/daemon_reload.rs` now fails a new legacy recovery request before creating
state, starting a child, or invoking PowerShell.  A helper scheduled by an
older binary is handled conservatively: after exact attempt ownership is read,
it records only the existing fixed `AUTHORITY_REQUIRED` terminal category and
returns failure.  It does not execute a script, replace a daemon, inspect a
tunnel, or use browser state.

The only mutable supervisor route is therefore the zero-choice closed
`operator supervisor activate` composition.  It retains the reviewed T-0223
authority chain: typed pipe-principal/runtime policy, explicit reviewed
supervisor-image capability, fixed startup definition, pinned protected root,
and `SupervisorInstallerWriterV1::fixed_policy()`.

`src/operator_facade.rs` removes the now-unreachable legacy PowerShell
recovery invocation builder.  It does not add an MCP recovery route.

## Current/LKG semantics

The stable activation transaction snapshots current and LKG separately under
the pinned protected root before mutation.  It prepares the reviewed image
inert, stages the fixed startup task disabled, commits the receipt, then
enables and verifies the exact task.  On a later scheduler failure it restores
the exact receipt snapshot before task compensation.  Existing protected-FS
tests cover present/present, absent/absent, present/absent, and absent/present
receipt shapes; LKG remains recovery material, not an implicit rollback claim.

The updated lifecycle regressions prove that status and activation source do
not borrow `daemon_reload`, the old PowerShell invocation, a script facade,
the versioned daemon mode, or browser authority.  The grammar also explicitly
rejects `operator supervisor recover`.

## Deterministic evidence

| Case | Evidence |
| --- | --- |
| New legacy recovery request | Fails before creating a recovery-attempt file or subprocess. |
| Old scheduled helper after upgrade | Retires only its matching attempt as `AUTHORITY_REQUIRED`; no old recovery executor is reachable. |
| Closed supervisor grammar | Accepts only status/preflight/activate and rejects recover plus caller authority arguments. |
| Stable status/activation source | Regression rejects legacy daemon/script/browser recovery references and requires fixed readiness/installer/snapshot APIs. |
| Interrupted transaction / LKG | Protected receipt snapshot regressions restore the exact current/LKG pair, including all independent present/absent combinations. |

## Verification

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| focused legacy-recovery closure test | PASS (1) |
| focused `supervisor_lifecycle` tests | PASS (11) |
| focused protected current/LKG snapshot tests | PASS (2) |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (860 tests; optional Python-advisor tests ignored) |
| `cargo build --all-targets --all-features` | PASS |
| configured `rust_full` / project verifier | Not configured or exposed in this workspace; no CatDesk MCP was used. |
| `git diff --check` | PASS at the bounded check; it reported only pre-existing broad-worktree CRLF warnings. |

Cargo emitted the pre-existing non-fatal warning that it could not canonicalize
`<USER_PROFILE>`; it did not cause a test or clippy failure.

## Attribution and boundaries

Task-attributable files are:

- `src/daemon_reload.rs`
- `src/operator_facade.rs`
- `src/supervisor_lifecycle.rs`
- this bundle

Those Rust files were already untracked in the intentionally dirty worktree,
so Git cannot isolate new hunks against a tracked baseline.  The named closed
legacy-worker removal and the two new deterministic tests are the narrow
reviewable boundary.  No unrelated file was cleaned, reset, staged, committed,
or published.

Host-live T-0223/T-0274 activation and continuity remain separately parked.
This ticket neither proves a live supervisor/listener/worker state nor permits
a direct manual use of historical version-coupled lifecycle scripts as a
substitute.  Any future host operation must use the reviewed stable lifecycle
and receive independent authorization.

## Independent-review request

Please independently review the removal of the detached legacy recovery
executor, the closed supervisor grammar/source guards, and the preserved
protected current/LKG compensation ordering before accepting this deterministic
T-0154-R1 slice.
