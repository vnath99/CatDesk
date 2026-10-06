# T-0141 / T-0037 Multi-project scheduler isolation

## Scope and result

This source-only slice closes the deterministic identity gap found in the
project registry/routing seams. A project id is no longer sufficient to select
another registered workspace's execution, target, or accounting identity.
No external repository, provider, browser, wake target, host lifecycle, or
Git remote was touched.

## Durable boundaries retained

| Boundary | Evidence |
| --- | --- |
| Concurrent projects within provider caps | Existing `schedule_mutating_worker` test grants `alpha` and `beta` Codex leases at a cap of two. |
| One writer per canonical workspace | The same test rejects another lease for `alpha` with `WORKSPACE_WRITER_LEASE_HELD`. |
| Exact project/workspace routing | New `project_for_workspace` requires both the registered project id and the canonical workspace to agree. |
| Exact thread routing | Existing registry validation keeps Codex thread bindings unique; host preparation now resolves the exact project/workspace binding before it can start the app-server transport. |
| Exact target routing | Wake target lookup and MCP target mutation both require the same exact project/workspace binding; there is no global target fallback. |
| Project-scoped evidence | Accounting replay for the same session/task now rejects a different project id rather than mutating the original record. |

## Changed production behavior

- `AutonomousProjectRegistryStoreV1::project_for_workspace` is the shared,
  canonicalized project/workspace resolver. It rejects an unknown project and
  a registered sibling workspace.
- `host_prepare_codex_app_server_thread` resolves that binding before spawning
  a Codex transport, so an unregistered or sibling `projectId` fails closed.
- `project_wake_target` rejects a project whose registered workspace differs
  from the caller's canonical workspace.
- Both MCP chat-target mutation forms preflight the exact workspace binding,
  leaving sibling target records unchanged on refusal.
- `ExecutionAccountingStoreV1::record_task_started` preserves the original
  project identity when replaying a session/task record and refuses a
  cross-project attempt.

## Regression evidence

| Case | Deterministic result |
| --- | --- |
| `alpha` and `beta` distinct workspaces at Codex cap two | Both leases granted. |
| Second writer for `alpha` | Refused as `WORKSPACE_WRITER_LEASE_HELD`. |
| `alpha` workspace asks for `beta` project | `project_for_workspace` refuses. |
| Workspace-bound wake lookup names sibling `beta` | Returns no target; it cannot select `beta`'s registered target. |
| Root-bound MCP target mutation names sibling project | Refused before mutation; sibling target fields remain absent. |
| Same session/task replay changes `alpha` to `beta` | Accounting rejects; the durable record remains `alpha` with one turn. |
| Thread reuse across projects | Existing registry test rejects it. |

## Verification

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| Focused project-registry, accounting, runtime, and supervisor tests | Passed (including new isolation regressions) |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed |
| `cargo test --all-targets --all-features --no-fail-fast` | Passed: 855 tests; expected ignored environment-dependent tests remained ignored |
| `cargo build --all-targets --all-features` | Passed |
| `git diff --check` | Passed |

The repository has an intentionally broad dirty/untracked worktree, so
repository-wide Git statistics are not task attribution. The attributable
T-0141 paths are:

- `src/delegated/autonomy_projects.rs`
- `src/delegated/autonomy_runtime.rs`
- `src/delegated/autonomy_supervisor.rs`
- `src/delegated/autonomy_accounting.rs`
- `src/delegated/autonomous_controller.rs` (fixture corrected to use its own
  durable `catdesk` project identity)
- this bundle

## Remaining live boundary

The remaining T-0037 acceptance is a separately authorized, read-only/no-op
cross-workspace canary against the durable live registry. It must prove the
same identities with real registered projects and must not mutate CatDesk,
BYOVD_DRIVER_PIPELINE, or BUG_BOUNTY_RECON_PLATFORM source. This task did not
access those workspaces or perform provider, browser-wake, target, tunnel, or
host operations.

## Status

`READY_FOR_INDEPENDENT_REVIEW` for the bounded deterministic source/test
slice. Independent review is still required before any live cross-workspace
canary or follow-on work.
