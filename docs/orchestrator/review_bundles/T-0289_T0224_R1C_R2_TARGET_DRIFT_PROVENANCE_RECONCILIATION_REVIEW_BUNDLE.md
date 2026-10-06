# T-0289 T-0224-R1C-R2 Target-Drift Provenance Reconciliation

## Scope

This is a repository-only diagnosis. No browser profile content was read, and
no browser/wake operation was invoked or retried. No wake target,
configuration, state, owner, Secure MCP/tunnel, host lifecycle, external
project, signing/provenance, dedicated-producer, or Git publication surface
was mutated.

## Exact durable T-0286 evidence

The T-0286 session is `COMPLETED_VERIFIED` and its corresponding fresh review
record is
`review-adc-t0286-post-target-repair-natural-wake-canary-20260830-7-independent_final_review`.
The exact schema-4 delivery entry for that record is:

| Field | Durable value |
| --- | --- |
| `status` | `OPERATOR_ATTENTION` |
| `attention` | `TARGET_DRIFT` |
| `browser_sent_at_unix` | absent |
| `message_sha256` | absent |
| `target_sha256` | absent |
| `receipt_schema_version` | absent |

The CatDesk project registry stored digest, independently recomputed registry
digest, and independently recomputed wake-config digest are all exactly
`d40b8d5b1995bb781f1c8e4f56bc6e6397f649678ad8ce9cb21232fdeba82d98`.
That is also the target digest returned by the guarded CAS. Only hashes were
examined; no target URL is reproduced here.

## Reviewed provenance path

1. `project_wake_target` in `src/delegated/autonomy_runtime.rs:723-740`
   resolves the project only through the workspace-bound registry, canonicalizes
   its target, recomputes its digest, and refuses it unless the stored digest
   matches. The compatibility config reader in `:858-902` is fail-closed and
   cannot select an alternate target.
2. `StableWakeOwner::dispatch_one` in `src/stable_wake_owner.rs:276-292`
   snapshots the protected target binding, claims the exact record, and asks
   `begin_submitting` to re-read and compare the protected binding before any
   adapter effect.
3. `StableWakeDelivery::begin_submitting` in
   `src/stable_wake_delivery.rs:158-169` calls
   `verify_protected_wake_target`; receipt acceptance at `:185` independently
   requires the same exact target and message binding.
4. The fixed adapter reloads the protected config and checks its target hash
   against the Rust boundary before constructing a browser sink
   (`scripts/stable_wake_browser_adapter.py:149-166`). A config/boundary
   mismatch returns `TARGET_NOT_READY`, not `TARGET_DRIFT`.
5. `CdpSink.readiness_reason` raises `TARGET_DRIFT` only after it has obtained
   a current page URL and found that URL not exact for the already validated
   expected target (`scripts/wake_bridge.py:454-467`). It occurs before submit,
   so no browser send receipt is created.

## Conclusion

No concrete repository logic or target-provenance defect is proven. The
registry and protected configuration agree with the guarded CAS digest, and
the reviewed code verifies that identity twice before adapter dispatch. The
only supported provenance for the exact durable `TARGET_DRIFT` is a later
host/runtime browser-page mismatch after the expected target had been bound.

The exact later gate is therefore **a separately authorized host/operator
natural-delivery observation using the already-bound owner**, after independent
review confirms this diagnosis. It must observe a fresh ordinary event with
schema-4 `SENT`, positive `browser_sent_at_unix`, receipt schema 1, and exact
record/message/target binding. It may not be replaced by manual wake, profile
inspection, or target mutation.

## Regression evidence and verification

Existing reviewed regressions cover the relevant fail-closed boundaries:

| Test | Proven behavior |
| --- | --- |
| `stable_wake_core::tests::protected_target_is_exact_read_only_and_drift_refuses` | Protected config target identity is read-only and any binding drift is rejected. |
| `stable_wake_delivery::tests::receipt_and_history_compatibility_fail_closed_or_classify_other_target` | A receipt/history bound to another digest never becomes an accepted send. |
| `stable_wake_owner::tests::success_pre_submit_and_ambiguous_paths_are_single_attempt_and_bounded` | Target/pre-submit/ambiguous outcomes remain bounded and do not create duplicate sends. |

No product source or test change is justified because the complete existing
chain already rejects the observed mismatch before submission.

| Command | Result |
| --- | --- |
| `cargo test --all-targets --all-features --no-fail-fast` | Passed: 860 tests. Expected platform-dependent ignored cases remained ignored; the command emitted a non-fatal `could not canonicalize path C:\\Users\\Volap` warning after successful test execution. |
| `git diff --check` | Passed (exit 0). The exact bundle is untracked within the intentionally dirty accumulated worktree, so no clean-tree baseline is asserted. |

## Attribution and independent review

T-0289 attribution is limited to this exact review bundle. The worktree is
intentionally dirty; no unrelated source or planning file is claimed by this
ticket.

**Status: REPOSITORY_PROVEN_HOST_RUNTIME_TARGET_MISMATCH.** T-0224 live
natural-wake acceptance remains open. Request ChatGPT independent final review
before any later host/operator observation. No manual wake was performed.
