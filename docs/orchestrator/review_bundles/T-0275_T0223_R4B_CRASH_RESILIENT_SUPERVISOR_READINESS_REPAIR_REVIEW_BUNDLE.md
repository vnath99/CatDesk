# T-0275 / T-0223-R4B — Crash-Resilient Supervisor Readiness Repair Review Bundle

## Review status

**IMPLEMENTATION + HOST VERIFICATION COMPLETE; READY FOR INDEPENDENT CHATGPT REVIEW.**

Provisional implementation conclusion: `READY_FOR_FRESH_T0274_HOST_ACTIVATION` if this bundle and the exact source are independently accepted. Controller/provider state is not acceptance authority.

## Trigger and predecessor disposition

T-0273 materially completed the activation-critical ProgramData migration to the shared pinned RootDirectory/no-follow filesystem authority, but independent ChatGPT review rejected its `SUPERVISOR_ACTIVATION_READY` conclusion for two bounded release blockers:

1. Fixed create-new temporary/receipt names and deterministic staging names could leave crash residue that wedges a later legitimate state/install commit.
2. Activation readiness did not source its decision from the actual T-0271 same-user/same-session pre-decode pipe authority or explicitly source stable-supervisor Secure-MCP/tunnel non-ownership from the actual runtime entrypoint.

The prematurely launched T-0274 host-live activation session was cancelled and is not authority for activation.

## Provider disposition and task attribution

The approved T-0275 autonomous session is `adc-t0275-t0223-r4b-crash-resilient-supervisor-readiness-repair-20260826`. Its first Codex/Terra-high turn terminated before mutation with a provider usage-limit diagnostic and reported retry time `2026-08-27 02:10 America/New_York`. The session remained `WAITING_FOR_CHATGPT`, provider turn count 1, with no authoritative session diff.

Because no provider mutation owner remained and the operator had already authorized direct CatDesk implementation when providers are blocked, ChatGPT continued the same approved bounded design through CatDesk. The broad repository was already intentionally dirty/untracked; whole-worktree diff is therefore not used as task attribution.

At the start of direct review the worktree already contained unattributed T-0275-like code: unique protected temp/staging helpers and source-linked principal/runtime readiness descriptors. ChatGPT did not automatically accept that material. It inspected the actual production call paths, found one additional crash-safety defect in the pre-existing repair, corrected it, added missing production-path regressions, and then ran independent verification.

### Direct T-0275-attributable repair in this review cycle

- `src/windows_protected_fs.rs`
  - Replaced process-local `PID + AtomicU64 counter` ephemeral naming with internally generated UUID-v4 sibling identities.
  - Reason: PID reuse plus counter reset after process crash/restart could reproduce a stale create-new name and recreate the exact liveness failure T-0275 is intended to close.
  - Added `protected_ephemeral_names_are_restart_independent_and_caller_unselectable`.
- `src/control_plane_supervisor.rs`
  - Added `stale_legacy_temp_and_stage_residue_cannot_wedge_state_or_install_retry` using the real state store and installer writer.
  - Added `activation_readiness_policy_gate_fails_closed_on_principal_or_runtime_drift` and structural ordering/non-ownership checks.
- `src/windows_supervisor_control_pipe.rs`
  - No semantic direct repair was required after source inspection; `cargo fmt` normalized formatting in already-present T-0275-like code. The production descriptor and actual pre-decode principal gate are independently inspected below.
- This exact review bundle was created as the required T-0275 artifact.

## Crash-residue repair

### Before T-0275

T-0273 moved authority-bearing operations to handle-relative/no-follow primitives but retained create-new identities whose names were repeatable. A crash after creating/writing a temporary state/receipt or staging object and before the same-parent commit could leave a safe object that permanently collided with the next retry.

### Current state/receipt commit authority

`ControlPlaneSupervisorStoreV1::save()` serializes validated state, opens the protected supervisor root, then calls `write_unique_regular_for_atomic_replace(...)`; the returned exact opened file handle is committed to `control-plane-supervisor.json` via `AtomicRegularWrite::commit_replace()` under the same pinned parent.

Supervisor current/LKG receipt commits use the same `write_unique_regular_for_atomic_replace(...) -> commit_replace(...)` pattern.

The generated temporary child name is internal-only:

- fixed product-selected kind (`file` or `stage`), never caller-selected,
- UUID-v4 generated inside `windows_protected_fs`,
- normalized by the existing protected component validator,
- bounded direct-child identity,
- create-new semantics remain fail-closed,
- collision causes a bounded retry,
- no stale pathname enumeration or deletion is introduced.

A stale sibling can remain after a crash, but a later process obtains a new restart-independent UUID child and therefore does not depend on reclaiming the stale object.

### Current installer staging authority

`SupervisorInstallerWriterV1::install_locked_protected()` clones the already pinned `versions` guard and calls `create_unique_renameable_child("supervisor staging")`. The staging child is opened/created relative to the pinned parent, the reviewed image is written/read/hashed through the retained guard, and final version commit uses `rename_direct_child_within_parent(...)` to the exact digest-derived version child.

A stale historical deterministic `.stage-<digest>` tree is no longer opened, reused, trusted, or deleted. It is inert and cannot block a new internally named staging child.

### No pathname-cleanup authority added

T-0275 deliberately does not add generic enumeration/delete/recursive-cleanup authority. The new regression proves stale state/receipt/stage fixtures remain present while the real state and installer operations succeed. This is the intended liveness model: stale residue is non-authoritative and non-blocking rather than reclaimed through an unsafe pathname delete path.

## Source-linked T-0271 principal readiness authority

`src/windows_supervisor_control_pipe.rs` owns `FixedPipePrincipalPolicyDescriptorV1` with the accepted production variant `SupervisorTokenUserAndSessionBeforeDecodeWithImageBinding` and a fail-closed `Unproven` variant.

This is not only a readiness-side assertion:

1. Production `serve_fixed_control_pipe()` obtains `fixed_pipe_principal_policy_descriptor()` and calls `require_fixed_pipe_principal_policy(policy)`.
2. The expected ordinary worker principal is derived from the supervisor's own OS token by `expected_worker_principal_from_supervisor_token(policy)`; no caller supplies user/session identity.
3. After the fixed named-pipe instance connects, production calls `peer_from_connected_pipe(&server, &expected_worker, policy)`.
4. That path derives the connected PID/token, requires exact `TokenUser` + `TokenSessionId`, classifies recovery identities as non-worker, and derives executable image evidence.
5. `peer_from_connected_pipe(...)` completes before `serve_one_fixed_control_request(...)`; request framing/JSON decode is therefore downstream of principal admission.
6. Recovery identities in the DACL do not obtain worker-control decoding authority.
7. Existing worker image/listener/reviewed-manifest and generation-CAS checks remain downstream and unchanged.

`assess_supervisor_activation_readiness_at()` consumes this same production descriptor through `readiness_policy_gate(...)`. If the actual descriptor is `Unproven`, readiness returns exactly `SUPERVISOR_PRINCIPAL_POLICY_UNPROVEN`.

The new regression checks the source ordering of the production peer gate before request dispatch in addition to calling the fail-closed policy gate directly.

## Source-linked stable runtime ownership / Secure-MCP non-ownership

`StableSupervisorRuntimeCapabilityV1` has the accepted production variant `FixedLocalFrontDoorPrivatePipeProtectedStateOnly` and a fail-closed `Unproven` variant.

The zero-argument stable binary `src/bin/catdesk-control-plane-supervisor.rs` consumes `stable_supervisor_runtime_capability_descriptor()` through `require_stable_supervisor_runtime_capability(...)` before `TcpListener::bind` and before either long-lived surface is run.

The stable binary's production ownership is limited to:

- fixed `127.0.0.1:3201` local front door,
- fixed private supervisor control pipe,
- protected stable-supervisor state/install store.

It does not import or invoke `cloudflared`, the official tunnel runtime/client, ngrok/openai tunnel lifecycle, or any Secure-MCP start/stop/reconfiguration path. The new source regression asserts the capability gate precedes the listener bind and rejects representative tunnel lifecycle ownership identifiers in this stable entrypoint.

Readiness consumes the same runtime capability descriptor. `Unproven` returns exactly `SUPERVISOR_RUNTIME_OWNERSHIP_UNPROVEN`.

This does not claim that the external Secure MCP runtime is currently healthy; it proves only the intended ownership boundary: the stable supervisor does not become a second tunnel owner. Live transport continuity remains a T-0274 host acceptance property.

## Fixed supervisor policy preserved

The T-0275 repair does not alter these accepted T-0270/T-0271 properties:

- stable front door host `127.0.0.1`, port `3201`, path `/mcp`,
- versioned local worker endpoint `http://127.0.0.1:3200/mcp`,
- fixed private Windows supervisor pipe,
- same-user + same-session worker admission before control-record decode,
- recovery identities unable to register/operate as worker,
- OS-observed worker executable/image evidence,
- reviewed-manifest binding,
- registration generation CAS / idempotency,
- preservation of the prior active backend on invalid replacement evidence,
- external Secure MCP remains separately owned.

## Adversarial / regression matrix

| Case | Production path exercised | Result |
| --- | --- | --- |
| Two internally generated file identities | `next_protected_ephemeral_component("file")` | Distinct UUID-backed bounded components |
| Internally generated stage identity | `next_protected_ephemeral_component("stage")` | UUID-backed bounded component |
| Caller attempts unsupported ephemeral kind | same helper | Refused |
| Historical PID/counter naming regression | source-level guard | No process-local counter or PID authority remains |
| Stale legacy `control-plane-supervisor.json.new` | real `ControlPlaneSupervisorStoreV1` save via backend registration | New state commits successfully; stale file remains inert |
| Stale legacy `supervisor-current.json.new` | real installer receipt commit | New current receipt commits successfully; stale file remains inert |
| Stale deterministic `.stage-<digest>` with partial image | real `SupervisorInstallerWriterV1` install path | New unique staging child commits reviewed image successfully; stale stage remains inert |
| No pathname stale cleanup | same crash-residue test | Stale fixtures deliberately remain; no deletion used as recovery authority |
| Principal descriptor valid | `readiness_policy_gate` + actual pipe descriptor | Accepted |
| Principal descriptor `Unproven` | same | Exact `PrincipalPolicyUnproven` |
| Runtime descriptor `Unproven` | same | Exact `RuntimeOwnershipUnproven` |
| Pipe principal gate ordering | production source ordering | `peer_from_connected_pipe` precedes request serve/decode path |
| Stable runtime gate ordering | production binary source ordering | capability gate precedes front-door bind |
| Stable binary tunnel ownership regression | production entrypoint source scan | No tunnel lifecycle ownership identifiers present |
| Existing reparse/replacement/same-parent rename/outside sentinel matrix | shared protected-FS and T-0273 suites | Remains passing in full suite |
| Reintroduced pathname authority in migrated supervisor functions | existing scoped static regression | Remains passing |

### Exact new focused tests

- `windows_protected_fs::tests::protected_ephemeral_names_are_restart_independent_and_caller_unselectable`
- `control_plane_supervisor::tests::stale_legacy_temp_and_stage_residue_cannot_wedge_state_or_install_retry`
- `control_plane_supervisor::tests::activation_readiness_policy_gate_fails_closed_on_principal_or_runtime_drift`

All three pass independently.

## Pathname-authority residue audit

A whole-file search for `Path::exists`, pathname `OpenOptions`, `fs::read/write/rename/remove_file/create_dir_all`, `.new`, and `.stage-` finds:

- test-fixture pathname operations used to create hostile/stale objects,
- the existing separately scoped reviewed-current-worker image evidence read near the early worker-evidence helper,
- static regression string literals.

The migrated activation-critical `ControlPlaneSupervisorStoreV1` state save/load and `SupervisorInstallerWriterV1` root/state/current/LKG/staging/version/image/receipt commit paths use the shared protected relative authority. There is no production fixed `.new` or deterministic `.stage-*` creation path remaining for those operations.

The existing scoped test `migrated_supervisor_authority_uses_only_protected_relative_operations` remains passing and scans the migrated authority bodies for pathname operations.

## Verification evidence

### Focused tests

1. `cargo test protected_ephemeral_names_are_restart_independent_and_caller_unselectable` — PASS.
   - First execution exposed a self-matching static-test literal only; the source regression was corrected to construct forbidden tokens from fragments. Product UUID behavior itself executed. Rerun passed.
2. `cargo test stale_legacy_temp_and_stage_residue_cannot_wedge_state_or_install_retry` — PASS.
3. `cargo test activation_readiness_policy_gate_fails_closed_on_principal_or_runtime_drift` — PASS.

### Formatting / lint

- `cargo fmt --all` — applied canonical formatting.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.

### Full Rust verification

- `cargo test --all-targets --all-features` — PASS, no test failures.
- CatDesk `verify_project` — `PASSED`:
  - `cargo fmt --check` PASS,
  - `cargo test` PASS,
  - `cargo build` PASS.
- `cargo build --all-targets --all-features` — PASS.
- `cargo check --release --bin catdesk-control-plane-supervisor` — PASS; optimized release-mode check completed in 21.50 seconds.
- `git diff --check` — PASS. The command emitted pre-existing LF→CRLF working-copy warnings across the dirty repository but no whitespace errors.

This closes the prior T-0273 uncertainty about release-mode supervisor compilation. A full live installation/activation is deliberately not performed by this ticket.

## Repository / mutation boundary

`git status --short` confirms the workspace remains broadly dirty/untracked from the long-running CatDesk program. T-0275 did not clean/reset, commit, push, merge, publish, or claim unrelated dirty content as task work.

No live mutation was performed to:

- `C:\ProgramData` stable supervisor installation/state,
- port 3201 supervisor listener,
- the private control pipe,
- the versioned CatDesk daemon/release,
- startup/Scheduler/services,
- browser/wake target/profile,
- externally owned Secure MCP/tunnel,
- signing/provenance/dedicated-producer surfaces,
- external projects,
- Git remote/publication state.

## Residual risk / next boundary

This ticket proves source/test readiness; it does **not** prove live Windows deployment continuity. The next boundary must be a fresh, separately reviewed T-0274 host-live activation/continuity acceptance using only the reviewed fixed installer/activation surface.

That host canary must fail closed on any elevation, identity, reparse, process/listener, generation, or protected-state ambiguity and must prove:

- stable 3201 remains continuously owned by the supervisor,
- ordinary daemon on 3200 registers through the principal-bound pipe,
- daemon restart/replacement changes backend generation without replacing 3201,
- stale generation/invalid principal/image evidence is rejected,
- worker outage is reported without losing the stable front door,
- rollback/recovery preserves the previously valid backend/state,
- externally owned Secure MCP is not restarted/duplicated/claimed by the supervisor,
- no manual browser wake is used as continuity proof.

## Implementation conclusion

`READY_FOR_FRESH_T0274_HOST_ACTIVATION`

This conclusion is provisional until ChatGPT independently reads this bundle and the exact source after verification. The parked Codex session/provider result is not acceptance authority.