# T-0030 Phase 2 Review Bundle

Status: implementation prepared for CatDesk's independent verification. This
worker did not commit, push, merge, deploy, publish, inspect credentials, or
perform a forced Codex exhaustion/Qwen fallback test.

## Implementation summary

Phase 2 builds on the existing uncommitted Phase 1/1B/1C worktree.

- The durable Codex-to-Qwen handoff now records bounded completed/pending
  tool-call provenance fields, mutation outcome hashes/summaries, Git
  porcelain status, changed paths, and an SHA-256 working-tree diff hash.
  The actual Codex CLI route does not receive CatDesk tools, so its first
  handoff correctly persists empty CatDesk tool-call sets rather than invented
  history; the persisted schema supports journal-derived records whenever
  present.
- `codex_app_server` is a supported, transport-injected protocol boundary.
  It reads only `account/rateLimits/read`, `thread/list`, and `thread/read`;
  it can consume the matching `account/rateLimits/updated` notification
  payload. No auth-file scraping, API key, billing, credit purchase, top-up,
  or reset-credit redemption operation exists in the module.
- Bounded telemetry is persisted in autonomous state. A confirmed reached
  limit sets `codexEligibleAfter` from the relevant reached window reset; a
  missing reset uses a 900-second conservative cooldown. A completed Qwen
  task only switches the next task back to Codex at/after that time and never
  interrupts an active Qwen turn.
- Durable planner metadata is stored beside the execution queue, including
  stable task/dependency IDs, acceptance criteria, allowed paths,
  verification profile, preferred-worker policy, escalation conditions,
  completion artifact references, and provider provenance. The runtime queue
  records `READY -> WORKER_RUNNING -> VERIFYING -> REPAIRING/COMPLETED` and
  preserves dependency gating across restart. Scope or architecture ambiguity
  remains an explicit `WAITING_FOR_CHATGPT` escalation.
- Stored thread targeting requires an exact normalized cwd. Explicit ID,
  title, or preview search returns one exact candidate, an ambiguity list,
  not-found, or concurrent-writer refusal. `thread/resume` is callable only
  for the exact non-owned resolution; it never resumes a candidate selected
  by heuristic ambiguity.

## State flow

```text
Codex app-server read/updated
          |
          v
bounded routing telemetry -- reached + reset --> codexEligibleAfter
          |                                         |
          |                                  no early Codex probe
          v                                         v
CODEX_CREDITS_EXHAUSTED --> durable handoff --> QWEN_FALLBACK_ACTIVE
                                      |                 |
                   Git status/paths/diff hash           | task boundary at/after eligibility
                   completed/pending provenance          v
                                      |            CODEX_PREFERRED (next task only)
                                      v
PLAN QUEUE: PLANNED -> READY -> WORKER_RUNNING -> VERIFYING
                                      |                 |
                                      +--> REPAIRING ---+--> COMPLETED_VERIFIED
                                      +--> WAITING_FOR_CHATGPT (scope/architecture ambiguity)
```

## Observability fields actually persisted when exposed

- rate-limit window name, used percentage, reached-limit flag, and reset
  timestamp;
- reached-limit classification and `codexEligibleAfter`;
- plan type, bounded credit/balance label, and earned-reset label;
- exact thread-selected model, reasoning effort, and bounded token-usage
  label from `thread/read`.

No live desktop app-server was contacted in this task, so availability of any
particular optional field is unclaimed. The default `installed-default` CLI
selector continues not to guess an effective model; persisted model/reasoning
values originate only from the supported thread payload when supplied.

## Files changed for Phase 2

- `src/delegated/codex_app_server.rs` — read-only telemetry parser/client and
  exact cwd-safe thread resolver/resume gate.
- `src/delegated/autonomy_state.rs` — telemetry, enhanced handoff provenance,
  and durable planner metadata.
- `src/delegated/autonomous_controller.rs` — authoritative handoff Git
  evidence, cooldown-aware task-boundary routing, and queue lifecycle states.
- `src/delegated/autonomy_supervisor.rs` — persisted initial plan metadata
  and combined plan/execution-queue status.
- `src/delegated/mod.rs` — app-server module export.

Existing Phase 1 files remain modified and were preserved as instructed.

## Deterministic tests and results

- `delegated::codex_app_server` — 3 passed: reset calculation/no billing
  method, read-only account/thread metadata, cwd/title ambiguity and
  concurrent-writer fail-closed behavior.
- `delegated::autonomy_state` — 13 passed: planner metadata/restart,
  reset/cooldown persistence, redacted completed/pending mutation provenance,
  and existing queue/restart coverage.
- `delegated::autonomous_controller` — 16 passed: Codex/Qwen handoff,
  no replay, transient limit behavior, restart, verification, and provider
  limits.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test` — 388 passed, 7 failed, 9 ignored. The failures are existing
  environment-dependent advisor executable absence and Windows `taskkill`
  access denial; no T-0030-focused failure occurred.
- `git diff --check` — passed (Git emitted existing CRLF conversion warnings).

## Authoritative diff summary

At final review capture, `git diff --binary HEAD | git hash-object --stdin`
returned `3988ccaf2512a2ba118eb64b1fbc34a8cfc8cf18` (Git blob hash of the full
current uncommitted patch). The patch includes the preserved Phase 1 changes
as well as Phase 2; untracked files are intentionally not represented in a
Git diff until staged. The handoff implementation separately computes a
SHA-256 of `git diff --binary --no-ext-diff HEAD` at the actual handoff point.

## Risks and limitations

- The app-server transport is intentionally injected by a desktop integration;
  this workspace does not start, discover, or authenticate an app-server.
- Git evidence uses an explicit unavailable marker for non-Git disposable test
  workspaces. A real repository handoff captures Git's own porcelain output
  and diff hash.
- Planner metadata persists the approved plan but this phase does not infer
  new tasks from worker output; expansion remains a ChatGPT decision.
- The requested live forced exhaustion/fallback acceptance was deliberately
  not run because the Codex allowance has reset.

## Recommended next phase

Wire the desktop's supported app-server transport into the autonomous runtime
and exercise it against an operator-approved disposable thread. Then attach
the existing CatDesk journal reader directly to the handoff producer so a
non-empty pre-handoff tool journal can be copied without a separate adapter.
