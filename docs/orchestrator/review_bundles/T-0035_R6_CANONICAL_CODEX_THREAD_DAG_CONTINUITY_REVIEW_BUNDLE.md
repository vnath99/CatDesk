# T-0035 R6: Canonical Codex Thread DAG Continuity Review Bundle

## Scope

This bundle records T-0085 only. It does not resume T-0084 and no browser,
daemon, tunnel, Scheduler, release, or Git-publication action was performed.

## T-0084 failure reconstruction

Durable task evidence is the approved T-0085 contract and plan at
`.catdesk/autonomy/adc-t0035-r6-canonical-codex-thread-dag-continuity-20260814/`.
The source path that made the reported failure possible was:

1. `host_prepare_codex_app_server_thread` selects the canonical project thread
   from the project registry (or trusted completed-session evidence), then
   persists it to `snapshot.provider_thread_id`. Its existing comparison of a
   session binding with the canonical project binding remains unchanged.
2. In `AutonomousControllerV1::run_once`, selecting the first ready DAG task
   made `selected_new_task` true. The prior `is_repair` condition was false
   for that new task, even though the host had already bound the thread.
3. The prior launch branch chose `start_turn` whenever `is_repair` was false.
   The Codex adapter maps `start_turn` to a fresh Codex CLI turn; only
   `resume_turn` passes the captured exact thread to `codex exec resume`.
4. `persist_handle` and the later provider-batch handling wrote the
   provider-reported identity into `snapshot.provider_thread_id` without an
   expected-thread comparison. A fresh returned identity could therefore
   overwrite the canonical binding.
5. On the next host preflight, the unchanged canonical project/session
   comparison correctly rejected the now-mismatched identities with
   `session Codex thread conflicts with the canonical project binding`.

The durable log corpus contains that fail-closed host diagnostic in
`.catdesk/logs/1786493266-9a07c2f8-2b07-4af2-96d3-29183987e4dc.log` and
`.catdesk/logs/1786629408-26dbbaaa-a834-4ded-ac61-ac441ae78db8.log`. This
bundle intentionally omits opaque thread identifiers.

## Root cause and correction

Task classification and provider transport had been coupled. A new task is
not a repair, but that fact says nothing about whether Codex is already bound
to a canonical project thread.

`selected_new_task` and `is_repair` now remain task-lifecycle concepts:
they reset or consume repair accounting, select repair instructions, and drive
repair events/budgets. A distinct `continue_canonical_codex_thread` condition
selects Codex transport. When Codex is active and preflight left a persisted
thread, the controller passes that exact identity through `resume_turn` for
both a newly selected task and a same-task repair. A genuinely unbound Codex
session alone uses `start_turn`. Ollama/Qwen launch behavior is unchanged.

## Fail-closed provider identity behavior

`AutonomousSessionSnapshotV1` now has a serde-defaulted,
`expected_codex_thread_id` field for the active, canonical-bound Codex turn.
`prepare_turn` records it before the provider launch. Both the returned handle
and each poll batch are compared before they can update durable state.

A mismatch appends only the bounded non-secret event
`codex_thread_identity_mismatch`, escalates with
`codex_provider_thread_identity_mismatch`, retains the canonical
`provider_thread_id`, and returns without a second provider launch.
`persist_handle` independently rejects a mismatched Codex handle as a final
write-path guard. Matching identities continue to persist normally. The Qwen
handoff explicitly clears the active Codex expectation.

## Changed files

- `src/delegated/autonomous_controller.rs`
  - Separates task/repair classification from Codex continuation selection.
  - Adds expected-thread validation on handles and poll batches plus the
    bounded escalation path.
  - Adds deterministic Codex transport test double and continuity tests.
- `src/delegated/autonomy_state.rs`
  - Adds the backward-compatible active-turn expected Codex identity.
- `src/delegated/autonomy_observability.rs`
  - Updates the complete snapshot test fixture.

## Deterministic coverage

New controller coverage proves:

- A canonical-bound initial task and A-to-B progression call resume with the
  same exact thread, never start a fresh Codex turn, and do not consume repair
  budget for the new B task.
- Same-task repair remains repair accounting while resuming the same thread.
- Mismatched identities from either the initial resumed handle or a poll batch
  fail closed without overwriting the durable canonical binding.

Existing focused coverage also passed for rate-limit/restart exact-thread
continuation, planner-reply rearm behavior, R2/R3 graph consistency, R4/R5
reviewer-loop ownership/replay protection, Terra/High routing, Qwen isolation,
and W13 wake receipt/idempotency behavior.

## Verification run

The following focused suites passed:

- `cargo test autonomous_controller --bin catdesk --no-fail-fast` (28 passed)
- `cargo test codex_app_server --bin catdesk --no-fail-fast` (16 passed, 1
  operator-local live probe ignored)
- `cargo test autonomy_runtime --bin catdesk --no-fail-fast` (24 passed, 1
  operator-local live probe ignored)
- `cargo test autonomy_supervisor --bin catdesk --no-fail-fast` (10 passed)

Repository-wide checks completed in this workspace:

- `cargo fmt -- --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test --no-fail-fast` passed: 518 passed, 18 ignored, 0 failed, plus
  both T-0035 marker integration tests passed.
- `git diff --check` passed.
- Cargo emitted its pre-existing non-fatal Windows path-canonicalization
  warning for `<USER_PROFILE>`; it did not affect any command status.

## Unchanged security and ownership boundaries

The host's canonical project/session comparison was not weakened. Terra/High
host gating, durable-first planner replies, reviewer generation ownership,
Qwen only after positively confirmed Codex credit exhaustion, accounting, and
W13 receipt rules retain their existing boundaries. No credential, opaque
thread identifier, browser state, external runtime, or Git publication action
is included here.

## Review gate

Independent review is required before any live action. After that review, run
a fresh live DAG canary to prove host-preflighted canonical-thread continuity
through a new-task transition. Do not resume the paused T-0084 session.
