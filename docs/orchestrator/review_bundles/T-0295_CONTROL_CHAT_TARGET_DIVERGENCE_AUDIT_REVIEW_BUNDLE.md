# T-0295 Control-Chat Target Divergence Audit Review Bundle

> **Superseded authority notice (T-0296, 2026-09-01):** The target mandate and
> digest supplied to this historical T-0295 audit were later superseded by a
> confirmed operator-authorized guarded update. The current canonical target is
> derived from coherent project-registry/effective-wake readback, not from the
> historical URL below. Preserve this bundle as a record of its bounded causal
> audit; do not use its old target/digest conclusion for current T-0224
> acceptance.

## Scope, immutable observation, and conclusion

This was a source, durable-artifact, and read-only-evidence audit. No target
setter, wake owner, browser, profile, registry writer, or protected
configuration writer was invoked.

The contract's read-only observation is that the current CatDesk project
registry and protected wake configuration agree on:

```text
https://chatgpt.com/c/6a932bca-64b8-83ea-8aff-fc7963272ac0
sha256: 4c26f2d04155e38f25473411a0d0c67cf02aff21c7317e9565553529165f5d2a
```

The mandated control chat is instead:

```text
https://chatgpt.com/c/6a9041b9-604c-83ea-a80e-815728e61421
```

The latest natural receipt is structurally schema-4 `SENT`, but is bound to
the former target. It is not T-0224 evidence for the mandated target.

**Causal conclusion: repository cause unproven.** The current pair is
coherent, so it is not evidence of a partial paired transaction. T-0292's
contract forbade its provider from accessing `.catdesk/wake-bridge`; its
session/journal records one completed provider turn, source/test work, and no
recorded target-set, wake, browser, or native GUI Apply action. Its tests use
injected fixture authority rather than the production authority. That excludes
the documented T-0292 test/provider path as evidence of a live update, but it
cannot prove the historical cause of the current coherent pair. No external or
operator action record establishing causation was available in the bounded
artifacts. Temporal proximity is not causal proof.

## Mutation map

| Writer / entrypoint | Authority and guard | Write ordering / recovery | T-0292 path? |
| --- | --- | --- | --- |
| `catdesk_wake_target_set` MCP tool -> `set_wake_target` | Closed request grammar; canonical credential-free ChatGPT URL; optional SHA-256 CAS; process-local target lock; safe fixed wake-config path | Atomic temp-file/rename replaces only `conversation_url`; no registry write or cross-store rollback because this is wake-only authority | No recorded call. It can create a mismatch if separately invoked, but cannot explain a coherent pair by itself. |
| `operator_set_wake_target` | Same wake-only canonicalization, optional CAS, lock, and atomic config writer | Same single-resource update; no project-registry write | Not used by the T-0292 GUI controller. |
| `AutonomousProjectRegistryStoreV1::bind_project_chat_target` via project/supervisor APIs | Project registration lock; canonical URL; optional registry digest CAS; supervisor path additionally requires project/workspace binding | Registry save only; no wake-config write or paired rollback | Not called by the recorded T-0292 provider path. It can create a mismatch if separately invoked. |
| `initialize_project_chat_target` / `migrate_catdesk_chat_target_if_unbound` | Registered project, canonical URL, registration lock; initialization rejects any existing target and migration acts only when unbound | Registry-only first-bind/migration; no wake-config write | Not T-0292; current bound state is ineligible for both. |
| `operator_update_designated_chat_target` | Canonical project + wake URL, mandatory displayed SHA-256 CAS, shared target lock, canonical workspace, coherent project/wake readback, exact single workspace project | Stages fixed wake-config CAS first inside the registry lock; then commits registry digest/url CAS; failed registry commit compensates the wake target only after the expected staged digest matches; failed compensation/readback becomes `SynchronizationFailure`; final coherent readback is required | This is the only production action used by T-0292's `ProductionDesignatedChatTargetAuthorityV1`, but no live GUI Apply was recorded. |
| GUI `DesignatedChatUrlControllerV1::apply` | Draft validation plus the controller's current authoritative digest | Draft edit is no-op; same value only refreshes; production Apply delegates only to the paired operator transaction | T-0292 tests inject `FixtureChatTargetAuthority`; test Apply changes fixture state only. Native controls are not run by those tests. |

`write_wake_config_atomic` is private to the wake writer. The T-0292 GUI has
no direct file writer, browser/wake call, or caller-selected config path.

## T-0292 reconstruction

| Durable artifact | Evidence | Audit interpretation |
| --- | --- | --- |
| Approved contract | Forbids `.catdesk/wake-bridge`, requires fixture-only implementation tests, and says the exact configured control chat remains unchanged absent explicit operator GUI Apply | Provider execution was not authorized to alter the protected wake configuration. |
| Session state | `COMPLETED_VERIFIED`, one Codex/Terra provider turn, zero repairs, canonical preserved thread | Completion is not target-mutation evidence. |
| Provider diagnostics/events | Source inspection, patch, fixture testing, verification, and documentation actions; no `catdesk_wake_target_set`, `operator_set_wake_target`, `operator_update_designated_chat_target`, wake, or browser execution event | No durable provider-path evidence of a live setter invocation. |
| T-0292 tests | Fixture authority tracks only test target/update calls; production GUI authority is not constructed by those tests | Fixture exercise cannot reach the live project registry or protected wake config. |
| Completion artifact / bundle | Verification passed; stated no real target mutation and no browser/profile access | Consistent with the contract and journal; not proof about a later external transition. |

The observed state could result from a separately authorized GUI Apply or from
another historical/operator-side transition. Neither possibility is proven by
the bounded repository artifacts. No source repair is justified under the
task's causal threshold.

## Acceptance impact and regression

T-0293/T-0294 remain fail-closed. A new deterministic preflight test constructs
an `ExactWakeDeliveryEvidenceV1` with all structural `SENT` fields present
(project, session, record, positive timestamp, 64-character message hash, and
receipt schema 1) but a non-mandated target hash. `wake_proof` classifies it as
invalid; evaluation returns `BLOCKED_BY_LIVE_ACCEPTANCE`,
`T0224_LIVE_EVIDENCE_INVALID`, and `t0224_accepted == false`.

This does not make a source fixture live evidence. It proves that a receipt
bound to `6a932...` cannot satisfy the mandate for `6a904...`. T-0223,
T-0222/T-0139, T-0152, and T-0155 remain unaccepted.

## Remaining operator/live boundary

The live pair must not be edited in this audit. If the mandate remains current,
a separately authorized operator action must reconcile the project registry and
protected wake configuration through the reviewed paired designated-chat CAS;
it must not directly write either file. Only afterwards can a separately fresh
ordinary final-review record naturally provide schema-4 `SENT`, positive send
time, receipt schema 1, and exact mandated record/message/target binding.

## Verification

| Command | Result |
| --- | --- |
| `cargo test core_host_acceptance_preflight --all-features` | Passed: 8 focused tests, including non-mandated structurally-sent receipt refusal. |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --workspace --all-targets --all-features` | Passed: 881 unit tests plus workspace integration targets. |
| `cargo build --workspace --all-targets --all-features` | Passed. |
| `rust_full` project profile | Satisfied by the all-target/all-feature workspace Rust profile; no separate configured profile was present. |
| `git diff --check` | Passed; existing dirty-tree CRLF warnings were non-fatal. |

The known non-fatal `C:\\Users\\Volap` canonicalization warning continued to
appear without affecting successful command exits.

## Attributable diff and prohibited-action audit

T-0295 changes are limited to:

- `src/core_host_acceptance_preflight.rs` (one deterministic fail-closed test)
- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `.catdesk/todo.md`
- this bundle

The repository was already intentionally dirty; no reset, clean, staging,
commit, or Git publication occurred. No live target/config/registry mutation,
browser/profile inspection or wake, supervisor/host mutation, Secure MCP/tunnel
action, external-project action, or signing/provenance/dedicated-producer work
was performed.

**Status: READY_FOR_INDEPENDENT_REVIEW — ROOT_CAUSE_UNPROVEN; T-0224 OPEN.**
