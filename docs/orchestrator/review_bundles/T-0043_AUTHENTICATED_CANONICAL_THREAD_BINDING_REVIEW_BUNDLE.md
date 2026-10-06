# T-0043 Authenticated Canonical Thread Binding Review Bundle

Status: `BLOCKED_DIRECT_APP_SERVER_CONTEXT_NOT_AUTHENTICATED`.

## Outcome

T-0043 freshly exercised the supported direct Codex app-server context that
CatDesk uses, after the reported operator login.  The process started and
initialized correctly, and the CatDesk loopback daemon remained healthy, but
the direct worker context is still unauthenticated.  The supported
`account/rateLimits/read` endpoint returned its authentication-required error,
and the direct executable's supported `login status` classified the same
context as not authenticated.

CatDesk therefore failed closed before selecting, creating, resuming, or
binding any thread.  No registry mutation, replacement-thread attempt,
continuity turn, daemon restart, credential inspection, API-key fallback,
billing action, or Git publication occurred.

## Preserved Workspace And Daemon Evidence

- The pre-existing uncommitted T-0030--T-0042 worktree was retained.  No
  branch, reset, checkout, commit, push, merge, PR, release, or deployment
  action was performed.
- `GET http://127.0.0.1:3200/` returned HTTP `200` during this run.
- A restart-handoff record remains present.  Since the existing daemon was
  healthy and an authenticated direct context was unavailable, no restart was
  justified or attempted.
- The direct executable was present but its path was redacted.  The optional
  `CATDESK_CODEX_HOME` handoff was absent, so the probe used the executable's
  default inherited context.  API-key environment variables were removed from
  the child context; no authentication material or raw account responses were
  read, printed, or persisted.

## Fresh Supported App-Server Probe

Canonical workspace:

`<USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop`

| Request | Bounded result |
| --- | --- |
| Direct `codex login status` | `NOT_AUTHENTICATED` |
| `initialize` | `SUCCESS` |
| `thread/list` with exact cwd, title search `Integrate Codex MCP for ChatGPT`, limit 32 | `SUCCESS`; 0 candidates |
| `account/rateLimits/read` | `ERROR: codex account authentication required to read rate limits` |

The supported protocol evidence distinguishes a working executable/protocol
from an unavailable account context.  The title-filtered zero result cannot
be treated as proof that a thread is absent while authenticated thread/history
access is unavailable.

## Thread, Registry, And Continuity Safety

No exact candidate was available to prove all required conditions: exact
thread ID, exact canonical cwd, not concurrently owned, and direct-input
eligibility.  In consequence:

- no existing `Integrate Codex MCP for ChatGPT` thread was bound;
- no replacement thread was created (the one-attempt safety budget remains
  unused);
- no `thread/read`, reconnect/list/read durability check, `thread/resume`, or
  `turn/start` continuity instruction was sent;
- `autonomy_project_registry_bind` was not called and no `catdesk` project to
  workspace to thread-ID mapping was written; and
- no subsequent CatDesk instruction could safely target a bound thread.

This preserves the exact-unowned-direct-input evidence gate and one-writer
rule rather than persisting a guessed or stale thread ID.

## Accounting

All account-dependent fields remain `UNKNOWN_NOT_CAPTURED` for T-0043:

| Field | Reason |
| --- | --- |
| Effective model / reasoning effort | No authenticated readable thread |
| Thread token usage | No authenticated readable thread |
| Rate-limit windows / used percentage / reset | Supported account endpoint rejected unauthenticated context |
| Plan or credit metadata | Same endpoint was unavailable; no inference made |
| Continuity turn / subsequent-thread proof | No direct-input-safe canonical thread |

No credits were purchased, reset, deliberately exhausted, or inferred from
token estimates.

## Required Operator / Platform Gate

The supported Codex login completed by the operator is not visible to the
direct executable context inherited by this CatDesk task.  Before a future
T-0043 continuation, the operator must make the authorized context available
to the CatDesk launch environment using the approved opaque
`CATDESK_CODEX_HOME` path handoff (or complete login in that direct context),
then relaunch CatDesk through the existing operator-controlled handoff if
needed.  The path and its contents must not be sent through MCP or exposed to
the worker.

After that gate is resolved, the task must fresh-run `initialize`,
`thread/list`, and `account/rateLimits/read`; only then may it resolve or make
at most one canonical replacement and bind it with exact-unowned-direct-input
evidence.

## Independent-Verifier Handoff

CatDesk reported that independent verification did not pass, with the only
bounded detail: `no bounded verifier detail was persisted`. The durable
T-0043 session directory contains only its contract, plan, queue, state, and
event journal; it contains no verifier artifact to diagnose a different
criterion. This bundle records that absence rather than inventing a failure
reason or treating it as authenticated-thread proof.

The repair for that report is limited to persisting the fresh direct
authentication outcome, the fail-closed non-mutations, and the local check
results below. CatDesk must perform a new independent verification and
authoritative diff capture after this handoff; neither is claimed here.

## Verification

- `cargo test codex_app_server -- --nocapture`: PASS (6 tests).
- `cargo test autonomy_supervisor::tests::project_registry_binding_is_canonical_evidence_gated_and_durable -- --nocapture`: PASS.
- `cargo test autonomy_runtime::tests::app_server_binding_requires_exact_workspace_thread_and_persists_bounded_telemetry -- --nocapture`: PASS.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --no-fail-fast`: 399 passed, 7 failed, 9 ignored.  The failures
  are environment-sensitive and match the recorded T-0042 baseline: three
  advisor tests could not start the configured advisor program, the related
  advisor-cancellation expectation failed, and three Windows process-tree
  cancellation tests received `ERROR: Access denied`.  The T-0043 focused
  app-server and registry checks passed in the same run.
- `git diff --check`: PASS (only existing line-ending warnings were emitted).

CatDesk independent final verification remains pending and is not claimed by
this bundle.
