# T-0031 Milestones 1–8 Review Bundle

Status: implementation and deterministic evidence are prepared for CatDesk
independent verification. No commit, push, merge, pull request, release,
deployment, provider credit purchase, authentication-file inspection, or live
forced-exhaustion test was performed by this worker.

## Completion and gap matrix

| Milestone | Result | Evidence / remaining external condition |
| --- | --- | --- |
| 1. Phase 2 audit | Prepared | Preserved T-0030 uncommitted work; Phase 2 focused suites passed before edits. The audit found the runtime transport and review-inbox gaps addressed below. |
| 2. App-server/thread binding | Deterministic implementation complete | `bind_codex_app_server_thread` accepts only an operator-provided transport, exact canonical workspace/title-or-ID resolution, non-owned thread, and optional explicit resume. It records bounded read-only telemetry. Live desktop transport acceptance remains operator-local and was not claimed. |
| 3. Review delivery substrate | Deterministic implementation complete | Project-aware durable unread inbox records are emitted for verified completion and ChatGPT escalations. List cursor/filter and idempotent acknowledgement are exposed. Polling is supported; unsolicited ChatGPT Web wake remains unavailable. |
| 4. Single-project DAG | Prepared | Existing durable queue/dependency, repair, restart, same-thread, escalation, and final-review gates were retained and focused controller tests pass. |
| 5. Multi-project registry | Deterministic implementation complete | Generic registry, unique project/thread binding, workspace leases, and independent Codex/Qwen concurrency limits are implemented above project-local state. |
| 6. Multi-project acceptance | Deterministic implementation complete | Disposable fixture tests demonstrate independent concurrent workspaces, same-workspace writer refusal, lease-expiry recovery, and project-scoped thread binding. No BYOVD or bug-bounty workspace was modified. |
| 7. Codex → Qwen | Deterministic implementation complete; live acceptance pending | Preserved deterministic handoff/Qwen tool-loop/return-at-task-boundary acceptance. No Codex allowance was intentionally consumed. Natural-exhaustion live acceptance is READY/PENDING. |
| 8. Resilience | Prepared | Existing targeted coverage covers restart/lost-session continuity, busy locking, rate limits, cancellation, Qwen unavailable/malformed output, verifier repair, and handoff validation; T-0031 adds corrupt-registry rejection and expired workspace-lease recovery coverage. |

## Architecture

```text
operator-local authenticated app-server transport
    -> exact cwd + title/ID resolve -> non-owned thread only -> optional resume
    -> bounded rate/model/reasoning telemetry -> project-local autonomous state

terminal completion/escalation -> review-inbox.json (unread, idempotent)
    -> existing hourly condition-watch / future event poller -> list/read/ack
    -> no direct unsolicited ChatGPT Web injection

global project registry -> provider concurrency budget -> workspace create-new lease
    -> project A state/artifacts/thread     project B state/artifacts/thread
    -> rejects a second mutating writer in the same canonical workspace
```

## Files changed for T-0031

- `src/delegated/autonomy_runtime.rs`: injected read-only app-server binding
  entry point and deterministic exact-thread test.
- `src/delegated/autonomy_state.rs`: bounded review-inbox persistence,
  project filter/cursor, atomic idempotent acknowledgement, and test.
- `src/delegated/autonomous_controller.rs`: emits inbox records only after
  completion evidence or persisted escalation.
- `src/delegated/autonomy_supervisor.rs`, `src/mcp.rs`: list/ack polling
  surface and schema/tool-list coverage.
- `src/delegated/autonomy_projects.rs`: generic registry, thread isolation,
  lease ownership/expiry, global provider limits, and disposable fixtures.
- `src/delegated/mod.rs`: module export.

T-0030 Phase 1/1B/1C/2 changes remain uncommitted and were audited rather
than rewritten. The authoritative worktree diff includes both preserved
T-0030 material and T-0031 additions.

## Verification record

Passed during this task:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- Focused: `delegated::codex_app_server` (3), `autonomy_state` (14),
  `autonomous_controller` (16), `autonomy_supervisor` (2),
  `autonomy_runtime` (1), and `autonomy_projects` (2).

The final full `cargo test` rerun completed with 392 passed, 7 failed, and 9
ignored. The failures are not attributed to this task: three require an
unavailable advisor program, three require Windows process-tree termination
permission, and one advisor-cancellation expectation is environment
timing-sensitive. The newly updated MCP tool-list test passes. No failure was
in the T-0031 focused paths.

`git diff --check` passed. The Git blob hash of the full uncommitted binary
diff captured immediately before this final evidence wording update was
`f9fea710c2f95938a9d2e03317996050815bb3c8`; CatDesk must recapture the final
authoritative hash during independent verification because this bundle itself
is an uncommitted artifact.

## Risks, limits, and exact remaining work

- CatDesk cannot create an unsolicited ChatGPT Web turn. The inbox is the
  durable producer side for the existing polling bridge and future triggers.
- The app-server transport is deliberately injected. This repository neither
  discovers a desktop server nor reads credentials/auth files; live binding to
  the stored `Integrate Codex MCP for ChatGPT` conversation requires a safe
  exact match supplied by the operator.
- A real Codex credit exhaustion event was not forced. The deterministic path
  is covered; live natural exhaustion stays READY/PENDING.
- CatDesk must independently rerun formatting, clippy, full tests, diff check,
  and recapture the authoritative diff/hash before recording final status.

## Independent verification request

CatDesk: please independently verify this worktree, including the external
platform limitations above, and provide final review. Do not infer a commit or
publication request from this bundle.
