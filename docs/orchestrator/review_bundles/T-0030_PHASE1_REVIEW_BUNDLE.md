# T-0030 Phase 1 Review Bundle

Status: implementation complete; CatDesk independent verification and final
review remain authoritative and have not been claimed by this worker.

## Implementation summary

Phase 1 adds a narrow durable route for an approved autonomous task:
`codex-cli` remains preferred; a confirmed authenticated plan/credit
exhaustion signal can continue the same logical task through loopback
Ollama/Qwen only. Generic 429/capacity signals retain the existing Codex
thread and backoff path. There is no OpenAI API, cloud, paid, browser,
DeepSeek, or other fallback candidate.

The controller persists provider route state and a versioned bounded handoff
before checking Qwen health/model availability. The handoff retains task and
contract provenance, ordered work, allowed/forbidden paths, opaque Codex
thread provenance, completed task IDs, budgets, verifier summary, and diff
reference fields. Validation rejects bounded secret markers. Qwen receives a
continuation instruction and does not receive Codex authentication state.

## Provider/state flow

```text
                           generic 429 / retry-after
CODEX_PREFERRED ------------------------------------> CODEX_TRANSIENT_RATE_LIMITED
       ^                                                        |
       |---------------- same Codex thread + backoff ----------|

CODEX_PREFERRED -- explicit plan/credit exhaustion --> CODEX_CREDITS_EXHAUSTED
                                                           |
                              durable redacted handoff + local health/model check
                                      +--------------------+-------------------+
                                      |                                        |
                                      v                                        v
                           QWEN_FALLBACK_ACTIVE                    QWEN_UNAVAILABLE
                           same logical task                       WAITING_FOR_CHATGPT
                           independent verification                no other provider
                                      |
                                      v
                         COMPLETED_VERIFIED only after
                         verifier + authoritative diff + final review
```

At a completed safe task boundary the route may prefer Codex again for the
next approved task. This never interrupts a healthy Qwen turn and does not
assert that usage has reset.

## Files changed

- `src/delegated/codex_cli.rs` — conservative availability classifier.
- `src/delegated/autonomy_state.rs` — durable provider route and bounded,
  validated handoff persistence.
- `src/delegated/worker_provider.rs` — Codex/Qwen-only route adapter and
  safe-boundary Codex preference seam.
- `src/delegated/autonomous_controller.rs` — handoff, escalation,
  continuity, and independent-verification preservation.
- `src/delegated/autonomy_runtime.rs` — live Codex + loopback-only Ollama
  composition.
- `docs/orchestrator/ARCHITECTURE_DECISION_WORKER_RUNTIME.md` and
  `docs/orchestrator/tickets/T-0030.md` — architecture and ticket record.

## Tests and results

- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Focused controller/state/classifier test suites — passed, including
  transient-no-fallback, credit handoff, Qwen-unavailable escalation,
  redaction persistence, restart continuity, and no duplicate Codex launch.
- `cargo test` — 381 passed, 9 ignored, 7 failed in unrelated existing
  advisor/job-manager integration tests. Failures were environmental:
  unavailable advisor program and Windows `taskkill` access denied under the
  sandbox. T-0030-focused tests passed.
- `git diff --check` — passed (line-ending warnings only).

## Authoritative diff capture

The worker captured the current working-tree patch using `git diff --binary`.

- SHA-256: `d10338e3382c10bb92bc97030c8edbda6fac0a3ec9199f2b23d2dc35eb42d9b6`
- No branch change, push, merge, PR, release, or deployment was performed.

CatDesk must recapture the authoritative diff and run its independent final
review before it records `COMPLETED_VERIFIED`.

## Risks and limitations

- Live Codex acceptance still requires the operator-local CLI executable
  prerequisite described by T-0028J; this bundle contains only deterministic
  implementation evidence.
- The handoff currently records controller-level completion/budget evidence;
  richer tool-journal checkpoint fusion is deferred to Phase 2.
- Qwen availability is checked at handoff/recovery. It is intentionally not
  probed mid-turn and Codex reset probing is not implemented.
- The full test suite has the sandbox-related failures noted above; they are
  outside the changed provider-route path.

## Recommended Phase 2

1. Join provider-router/journal tool-call state directly into the durable
   autonomous handoff and validate an authoritative Git status/diff hash at
   handoff time.
2. Add reset-time-aware Codex re-eligibility checks at approved task
   boundaries and exercise multi-task queue advancement.
3. Add an explicit ChatGPT escalation/reply acceptance test across a persisted
   Qwen handoff and a live disposable local-Qwen/Codex operator test when
   authorized.

## ChatGPT review correction / Phase 1B

### Finding recorded before the correction

The initial Phase 1 controller constructed every `ProviderTurnRequestV1` with
an empty `tool_definitions` vector. That was correct for `CodexCliProviderV1`,
but it also meant the local Ollama/Qwen request exposed no CatDesk coding
tools. The controller then accepted a terminal Qwen batch by moving straight
to independent verification; normalized Qwen `ToolCall` events were not
dispatched through CatDesk's bounded executor. A fake provider could therefore
claim completion without demonstrating local code inspection, patching, or
verification.

### Correction

- `AutonomousQwenToolLoopV1` now wraps the existing
  `IntegratedDelegatedService`; it does not add a filesystem or shell
  executor. Calls retain the integrated service's policy checks, journal
  transitions, tool-call IDs, outcome handling, patch preview/apply flow, and
  non-replay protection.
- The controller supplies CatDesk definitions only when the persisted route is
  active local Qwen. The exposed surface is exactly `read`, `search`,
  `patch.preview`, `patch.apply`, `patch.compare`, `diff.actual`, and
  `verify.run`; `job.*`/shell authority is excluded. Codex turns still receive
  an empty definition list.
- Path-bearing Qwen calls are additionally checked against the autonomous
  contract allowlist before dispatch. Missing, unknown, or out-of-scope calls
  fail closed.
- On a Qwen tool event the controller journals and executes it through the
  integrated service, persists the bounded tool result history, and queues a
  model continuation. It never skips directly to final verification.
- A Qwen completion claim is rejected until CatDesk has recorded a passing
  `verify.run`, `diff.actual`, allowed diff paths, and no unknown tool outcome.
  Only after that gate does the controller run its separate contract verifier,
  authoritative diff capture, and final review.

### Deterministic proof

- `delegated::autonomous_controller::tests::credit_exhaustion_hands_same_task_to_qwen_without_replaying_codex`
  injects a Codex credit-exhaustion signal, checks the durable same-task Qwen
  handoff, proves Codex had no tools, proves Qwen receives only the seven
  bounded definitions, and proves an unsupported bare Qwen completion remains
  queued rather than completing the task.
- `delegated::autonomous_qwen_tools::tests::deterministic_qwen_tool_path_executes_read_preview_apply_verify_and_diff`
  uses a disposable Git/Cargo fixture and the production integrated dispatcher
  to execute normalized Qwen calls in this order: `read`, `patch.preview`,
  `patch.apply`, `verify.run`, and `diff.actual`. The completion gate passes
  only after those artifacts exist; a second `patch.apply` with the completed
  call ID is rejected as a replay.

This is deterministic production-equivalent evidence: the controller handoff
and provider-specific request surface are exercised without requiring an
operator-local model, and the actual CatDesk executor/journal/policy path is
used for the fixture mutations. Existing ignored Ollama integration smoke
tests remain opt-in through `CATDESK_LIVE_OLLAMA_MODEL`; no live local Ollama
smoke was run for this correction.

### Remaining limitations

- The Phase 1B proof uses scripted normalized Qwen events rather than a live
  model, so native tool-call parser/model behavior remains covered only by the
  existing ignored local-Ollama tests.
- The controller rehydrates the Qwen tool-loop journal/history after process
  restart; cross-process provider-session transcript restoration remains a
  later hardening area.

## ChatGPT review correction / Phase 1C

### Finding

`AutonomousQwenToolLoopV1::execution_contract` converts autonomous absolute
allow/deny paths to execution-contract-relative strings with `strip_prefix`.
When an allowed path is exactly the workspace root, that conversion yielded an
empty string. `validate_contract_path` correctly rejects empty paths, so
integrated-service recovery failed even though workspace-root allowlists are a
valid autonomous contract scope.

### Correction

- The adapter now emits `.` as the explicit execution-contract representation
  for the workspace root; empty paths remain invalid.
- Execution-contract validation accepts only an exact `.` root sentinel and
  continues to reject `.` as a component of any other path.
- Integrated diff/patch scope matching recognizes `.` as workspace-wide while
  retaining forbidden-path precedence. Autonomous canonical containment checks
  still gate every Qwen path-bearing call before the integrated service sees
  it.

### Deterministic proof

- `delegated::autonomous_qwen_tools::tests::deterministic_qwen_tool_path_executes_read_preview_apply_verify_and_diff`
  now uses `allowed_paths = [workspace]` and
  `forbidden_paths = [workspace/.git]`. It proves adapter recovery, a bounded
  `src/lib.rs` read, patch/verify/diff flow, and rejection of `.git/config`
  plus `../outside.txt`.
- `delegated::contracts::tests::workspace_root_scope_uses_dot_without_allowing_dot_components`
  accepts the exact `.` root scope and rejects `./src`.

### Verification record

- Focused root-scope/Qwen recovery and execution-contract tests passed.
- `cargo fmt --check` and
  `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test` produced 383 passed, 9 ignored, and the same 7 unrelated
  environment failures recorded above: the advisor test program was absent
  and Windows process-tree termination was access-denied by the sandbox.
- `git diff --check` passed (line-ending warnings only).

This is a representation correction only: it adds no provider, shell, cloud,
paid, API, Git publication, or broader filesystem authority.
