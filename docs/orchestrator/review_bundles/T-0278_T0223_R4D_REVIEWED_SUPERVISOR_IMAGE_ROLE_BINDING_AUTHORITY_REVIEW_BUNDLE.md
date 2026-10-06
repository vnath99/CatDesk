# T-0278 / T-0223-R4D reviewed supervisor-image role-binding authority

## Disposition

This source-only slice closes the distinct supervisor-image binding prerequisite without adding a signing key, envelope, manifest, provenance producer, startup mutator, or host activation path.

**Status: READY FOR INDEPENDENT CHATGPT REVIEW.** This is not startup authority, stable-supervisor installation, or host-live acceptance.

## Existing signed-payload trust chain reused

The accepted reviewed-main-image path remains fixed to `C:\Program Files\CatDesk\CatDesk.exe`. Its accepted envelope is read with the existing no-follow regular-file opener, verified against the compiled existing public trust root and fixed bootstrap policy, and bound to the exact fixed image through measured SHA-256 and length from the safely opened object. The pre-existing `verified_current_reviewed_main_image_digest()` remains a digest-only bridge for ordinary worker registration.

No accepted generic multi-artifact manifest or distinct supervisor envelope existed. T-0278 therefore adds no second trust root, key, signing flow, envelope, producer, or provenance authority.

## Explicit role binding

`reviewed_build::verified_reviewed_stable_supervisor_image()` is the only production constructor for `ReviewedStableSupervisorImageV1`.

| Property | Fixed value / rule |
| --- | --- |
| Role purpose | `stable-supervisor-runtime-v1` |
| Source | Exact already-opened `C:\Program Files\CatDesk\CatDesk.exe` after accepted envelope verification |
| Evidence | Envelope signature/policy plus exact opened-object SHA-256, length, and stable identity |
| Result | Opaque authenticated bytes, digest, and length for the fixed installer only |
| Prohibited authority | Caller path, bytes, digest, handle, filename, CWD, PATH, sibling discovery, `target/release`, `current_exe`, or worker digest bridge |

Bytes are read from the same retained file object used for measurement, then remeasured before the role capability is returned. The existing Windows share mode prevents write/delete pathname substitution while that object is read. A capability is not constructible by public/caller input.

### Worker versus supervisor semantics

| Consumer | Accepted artifact evidence | What it receives | Authority |
| --- | --- | --- | --- |
| Ordinary 3200 worker registration | Existing accepted envelope + fixed opened image | Digest only through `verified_current_reviewed_main_image_digest()` | Worker registration identity only |
| Stable supervisor installer | The same envelope and independently re-opened/measured fixed image, then explicit role binding | Opaque bytes + exact digest/length | Fixed `stable-supervisor-runtime-v1` installer input only |

The worker digest bridge alone is never consumed by supervisor lifecycle code.

## Shared fixed supervisor runtime entry

The authenticated CatDesk payload now has one zero-choice entry:

```
CatDesk.exe --catdesk-control-plane-supervisor
```

It accepts no extra argument. `run_fixed_stable_supervisor_runtime()` is shared by that entry and the standalone zero-argument `catdesk-control-plane-supervisor.exe` wrapper. This prevents drift between the two executable surfaces.

The shared runtime requires the accepted runtime capability, opens only fixed supervisor state read-only, binds only `127.0.0.1:3201`, and on Windows runs the fixed private control pipe and front door with `try_join!`; either terminal surface ends the process. It does not configure/start/stop Secure-MCP or a tunnel, and it does not mutate the ordinary 3200 worker lifecycle.

## Lifecycle effect and preserved fail-closed ordering

`supervisor_lifecycle::production_reviewed_supervisor_image()` now consumes only the explicit role capability. A valid role capability moves preflight beyond `REVIEWED_SUPERVISOR_IMAGE_UNAVAILABLE` to the next actual fixed gate: `SUPERVISOR_STARTUP_AUTHORITY_UNAVAILABLE`.

`fixed_startup_authority()` remains unavailable. No SCM, Scheduler, PowerShell, cmd, generic process command, auto-elevation, or UAC bypass was added. Activation returns before installer mutation while that gate is unavailable. The accepted T-0277 typed principal/runtime outcomes and the T-0275 protected-state-before-installer activation ordering remain unchanged.

## Same-principal / no-new-privilege analysis

The installer may place the full authenticated CatDesk payload under the fixed supervisor filename because the installed executable can invoke only the compiled fixed supervisor role, under the same OS principal. The role grants no privilege the authenticated CatDesk image did not already possess: it owns only fixed local 3201, the private control pipe, and protected local supervisor state/install lifecycle. It does not add external Secure-MCP/tunnel, worker, browser, wake, or arbitrary command authority. A future change that weakens the fixed role grammar or introduces a distinct privileged principal must fail closed and require a separate review.

## Hostile / mismatch matrix

| Case | Result |
| --- | --- |
| Exact signed envelope + exact safely opened image | Role capability accepted |
| Signed envelope payload digest mismatch | Refused |
| Signed envelope length mismatch | Refused |
| Directory/non-regular candidate | Refused by safe opener |
| File reparse redirect (where constructible) | Refused before role binding |
| Missing/malformed role binding | `REVIEWED_SUPERVISOR_IMAGE_UNAVAILABLE` |
| Valid role binding, startup authority unavailable | `SUPERVISOR_STARTUP_AUTHORITY_UNAVAILABLE`; no installer mutation |
| Extra supervisor runtime argument | Refused |
| Worker daemon/operator invocation | Not treated as supervisor role |

## Verification

| Gate | Result |
| --- | --- |
| Focused reviewed-image role-binding tests | PASS (2 tests) |
| Focused lifecycle tests | PASS (10 tests) |
| Focused shared runtime-entry and dual-surface tests | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (815 tests; existing opt-in Python advisor tests explicitly ignored) |
| `cargo build --all-targets --all-features` | PASS |
| `cargo check --release --bin catdesk` | PASS |
| `cargo check --release --bin catdesk-control-plane-supervisor` | PASS |
| Project `rust_full` / `verify_project` harness | Not configured/discoverable in this workspace; not inferred |
| Source authority scan | PASS; scoped runtime/binding guards reject worker bridge and path/process shortcuts |
| `git diff --check` | PASS (pre-existing line-ending warnings only) |

Cargo emitted its existing `could not canonicalize path <USER_PROFILE>

## Narrow attributable surface

T-0278 implementation changes are limited to:

- `src/reviewed_build.rs` — fixed role-binding capability and isolated adversarial tests.
- `src/supervisor_lifecycle.rs` — role-capability-only image acquisition and lifecycle regression.
- `src/control_plane_supervisor.rs` — shared fixed supervisor runtime and closed entry parser/tests.
- `src/bin/catdesk-control-plane-supervisor.rs` — wrapper delegated to the shared runtime.
- `src/main.rs` — fixed no-extra-argument runtime entry dispatch.
- This bundle.

The repository remains broadly dirty and several listed source files are inherited untracked prior-task surfaces, so Git cannot mechanically isolate earlier lines inside them. No clean/reset/stage/commit operation occurred, and no unrelated path was edited for this ticket.

## Prohibited actions not performed and residual prerequisite

No live ProgramData, 3201 listener, private pipe, startup, SCM, Scheduler, worker/release, browser/wake, Secure-MCP/tunnel, real `.catdesk`, signing/provenance, or Git mutation occurred.

The remaining single prerequisite is an independently reviewed **fixed native startup authority**. Only after that separate source review may any fresh host-live supervisor activation be considered. Please perform independent ChatGPT review of this role binding before starting that future ticket.
