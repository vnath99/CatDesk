# T-0032–T-0038 Live Integration Review Bundle

Status: `OPERATOR_GATE` — the safe stop condition has been persisted. This
bundle distinguishes deterministic implementation evidence from live evidence;
it does not claim that the updated daemon, app-server transport, Codex thread,
or external project workspaces were live in this worker session.

## Status matrix

| Ticket | Status | Evidence / exact limitation |
| --- | --- | --- |
| T-0032 | `OPERATOR_GATE` | Updated CatDesk debug binary built successfully. T-0031 surfaces and the new `autonomy_execution_accounting` surface compile. The current MCP daemon predates those changes; no continuity-preserving reexec exists, so it was not stopped from its own control connection. |
| T-0033 | `PENDING` | Requires an operator-provided supported Codex app-server transport after relaunch. No title/cwd candidate query, binding, read-only continuity check, or resume was attempted. |
| T-0034 | `DETERMINISTIC_PASSED` | Durable versioned task/session ledger, bounded MCP report, authoritative-snapshot-only delta calculation, and historical T-0031 backfill were implemented and focused-tested. No live post-task app-server usage snapshot was exposed in this session. |
| T-0035 | `PENDING` | The required 4+ task real CatDesk DAG cannot be started against the old daemon. No acceptance source/fixture work was fabricated. |
| T-0036 | `OPERATOR_GATE` | Real-project registration is deferred until the live daemon is restored. This worker did not access the BYOVD or Bug-Bounty workspaces, and made no edits there. |
| T-0037 | `OPERATOR_GATE` | Real multi-project scheduling requires the updated live runtime and operator-approved cross-workspace scope. No leases, provider calls, or cross-project operations were attempted. |
| T-0038 | `DETERMINISTIC_PASSED` | This consolidated evidence bundle and exact relaunch gate are present. Live acceptance remains pending. |

Natural Codex-exhaustion → local-Qwen live acceptance remains `PENDING`; no
allowance was intentionally consumed or exhausted.

## T-0032 build and relaunch gate

- Build performed: `cargo build` completed successfully for the current
  worktree, producing `target/debug/catdesk.exe`.
- The compiled source includes the T-0031 app-server binding, durable review
  inbox, generic project registry/scheduler, and now the execution-accounting
  module/MCP surface.
- [operator_relaunch_updated_daemon.ps1](../../../scripts/operator_relaunch_updated_daemon.ps1)
  validates the updated build, workspace, and presence (not value) of the
  operator-local `CATDESK_CODEX_CLI_EXECUTABLE`. It never reads credentials,
  starts a paid/cloud fallback, inspects/stops an active CatDesk process, or
  changes tunnel configuration.
- The exact operator action is in
  [OPERATOR_GATE_T0032_DAEMON_RELAUNCH.md](../OPERATOR_GATE_T0032_DAEMON_RELAUNCH.md).
  Complete it outside the existing MCP control connection, reconnect the
  existing Secure MCP tunnel through the normal operator workflow, then verify
  `autonomy_execution_accounting` appears in the tool list before resuming.

## Execution accounting (T-0034)

`src/delegated/autonomy_accounting.rs` persists
`.catdesk/autonomy/execution-accounting.json`. Each task record has project,
task, CatDesk session and provider session/thread identity; start/end/elapsed
milliseconds; provider/model/reasoning metadata when observed; provider turns,
normalized events, CatDesk tool calls, retries, repair cycles; verification
timing/result/reference; final diff/review references; and bounded pre/post
Codex app-server snapshots.

The report is read-only MCP operation `autonomy_execution_accounting` with a
session filter and aggregate elapsed/turn/tool/retry/repair totals. It does not
return raw provider diagnostics or credentials.

Usage policy:

- A rate-limit used-percent delta is reported only when the ledger has both
  authoritative snapshots for the same named window and reset timestamp.
- Credit usage remains `UNKNOWN_NOT_CAPTURED` unless a supported app-server
  supplies directly comparable authoritative evidence. Token estimates and the
  cosmetic dollar meter are never used.
- Qwen/Ollama tasks record their active provider/model and elapsed wall-clock
  time through the same ledger; no Qwen task was invoked here.

### Historical T-0031 backfill

The durable ledger contains `t0031-historical` for session
`adc-t0031-milestones1-8-20260808`:

| Metric | Recorded value |
| --- | --- |
| Elapsed wall-clock | `630023 ms` |
| Provider | `codex-cli` |
| Provider turns | `1` |
| Normalized provider events | `91` (captured diagnostic cursor) |
| CatDesk tool calls / retries / repairs | `0 / 0 / 0` |
| Verification | `PASSED` |
| Credit usage | `UNKNOWN_NOT_CAPTURED` |

No pre/post snapshot was retrospectively invented.

## Thread and project bindings

No live bindings have been written by this phase.

| Project | Canonical workspace | Thread binding | Scheduler acceptance |
| --- | --- | --- | --- |
| CatDesk | `<USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop` | Pending exact app-server resolution of `Integrate Codex MCP for ChatGPT` by canonical cwd + title; fail closed on ambiguity/busy ownership. | Pending relaunch |
| BYOVD Pipeline | `<USER_PROFILE>\OneDrive\Desktop\Projects\BYOVD_DRIVER_PIPELINE` | Pending; no discovery was attempted. | Pending relaunch and operator-authorized cross-workspace runtime scope |
| BUG_BOUNTY_RECON_PLATFORM | `<USER_PROFILE>\OneDrive\Desktop\Projects\BUG_BOUNTY_RECON_PLATFORM` | Pending; no discovery was attempted. | Pending relaunch and operator-authorized cross-workspace runtime scope |

The generic T-0031 registry’s one-writer workspace lease and unique project
thread-binding protections remain deterministic coverage only for this phase.

## DAG, review inbox, and wake behavior

No live DAG was run. Existing durable queue/dependency, repair,
restart-recovery, `WAITING_FOR_CHATGPT`, and idempotent review-inbox behavior
remain covered by prior deterministic tests. The intended live acceptance will
use only bounded disposable/documentation fixtures, prove `A → {B,C} → D`, and
leave architectural ambiguity in `WAITING_FOR_CHATGPT`; it must not expand its
own scope. Review records are polling-based; CatDesk cannot inject an
unsolicited ChatGPT Web turn.

## Verification evidence

Completed during this worker run:

- `cargo build` — passed (both before and after the accounting changes).
- `cargo check` — passed.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test autonomy_accounting --no-fail-fast` — passed (2 tests), including
  the guarantee that an empty read-only accounting report does not create a
  ledger file.
- `cargo test autonomy_supervisor --no-fail-fast` — passed (2 tests).
- `cargo test mcp::tests::multi_tools_list_exposes_run_command_mv_without_move_path_tool -- --nocapture`
  — passed; this confirms the accounting report is advertised by the normal
  multi-tool MCP list.
- PowerShell parser validation of the relaunch helper — passed.
- JSON parse validation of `.catdesk/autonomy/execution-accounting.json` —
  passed.
- Full `cargo test` completed: 393 passed, 7 failed, 9 ignored. The seven
  failures match known environment limitations: three advisor tests require an
  unavailable advisor program; three process-tree cancellation tests received
  Windows `Access denied`; and one configured-advisor cancellation expectation
  then lacked the external advisor program. The new accounting and MCP-list
  focused tests passed. No failure was attributed to this phase’s new paths.
- `git diff --check` — passed. It must still be repeated by CatDesk independent
  review because Git does not include untracked files in that check. The
  source-tracked binary-diff hash captured in the final worker run was
  `21a8ca125624130c5c589a1fd1c25cbd1d55e448`;
  it excludes untracked worktree artifacts, so independent verification must
  recapture the authoritative full-worktree evidence after review.

## Risks and remaining actions

1. Perform the exact T-0032 operator relaunch gate, reconnect transport, and
   prove the updated tool list/build identity.
2. Provide the supported app-server transport only after that reconnect; resolve
   the CatDesk thread read-first by exact cwd/title, reject ambiguity/busy
   ownership, and persist an exact binding only on a single non-owned match.
3. Run the bounded live DAG, then register and schedule the real projects using
   read-only/no-op/disposable tasks. Preserve one mutating writer per workspace.
4. Capture authoritative pre/post app-server snapshots around live turns when
   the transport exposes them. Leave unavailable fields explicitly unknown.
5. Run the required final verification and independent final review. No commit,
   push, merge, PR, release, deployment, billing change, API-key fallback, or
   forced allowance exhaustion is authorized.
