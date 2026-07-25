# Integrated Worker Closure

Status: T-0023A closure evidence
Date: 2026-07-25

## Integrated Service

`src/delegated/integrated.rs` composes the delegated-run primitives into one
CatDesk-owned service:

- execution contract and run state;
- durable journal;
- bounded context assembler;
- provider routing registry;
- Ollama/Qwen adapter integration;
- CatDesk tool dispatcher;
- patch preview/apply/compare/diff engine;
- coordinator final-review gate;
- verification runner;
- long-running job manager;
- supervisor event/checkpoint surface.

The provider receives only tool schemas. It does not receive direct filesystem,
shell, Git, patch, or process authority.

## Provider-Visible Tools

The closure tool surface is:

- `read`
- `search`
- `patch.preview`
- `patch.apply`
- `patch.compare`
- `diff.actual`
- `verify.run`
- `job.start`
- `job.status`
- `job.poll`
- `job.cancel`

`job.*` execution flows through the existing CatDesk command safety policy and
job manager. Verification flows through CatDesk verification detection and
bounded output summaries.

## Deterministic CI Path

`cargo test delegated::integrated -- --nocapture` runs the deterministic
fake-provider integration path. It proves:

- integrated service startup;
- at least one completed tool result;
- journal-backed restart recovery;
- completed tool-call replay blocking after recovery;
- patch preview/apply;
- verification to `COMPLETED_VERIFIED`;
- final review with no push or merge;
- job start/status/cancel tools.

## Live Qwen Path

The opt-in live closure test is:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP "catdesk-target-t0023a"
cargo test delegated::integrated::tests::ollama_qwen_live_model_tool_model_closure -- --ignored --nocapture
```

The temporary target directory is used because Windows Application Control
blocked the default test executable under the repository target directory during
this run.

The successful live sequence was:

1. Qwen requested `read`.
2. CatDesk executed `read` and returned bounded source content.
3. Qwen requested `patch.preview` for `patch-live-1`.
4. CatDesk previewed and applied the first patch.
5. Qwen requested `verify.run`.
6. CatDesk verification failed and returned bounded failure output containing
   assertion details.
7. Qwen requested revised child `patch.preview` for `patch-live-2`.
8. CatDesk compared and applied the revised child patch.
9. Qwen requested `verify.run`.
10. Verification passed.
11. CatDesk captured the authoritative Git diff.
12. The coordinator produced `COMPLETED_VERIFIED`.

Raw redacted transcript evidence is included in the external review bundle.

## MCP Transport

T-0023A exposes the supervisor operations through the real CatDesk MCP
`tools/list` and `tools/call` handlers. The focused MCP test invokes:

- `delegated_run_create`
- `delegated_run_validate`
- `delegated_run_start`
- `delegated_run_status`
- `delegated_run_events`
- `delegated_run_get_patch`
- `delegated_run_get_diff`
- `delegated_run_get_final_review`
- `delegated_run_cancel`

The MCP closure surface is process-local and loopback-only when used through
the existing hardened headless MCP server posture.

## Deferred

Browser providers remain deferred. OpenClaw remains optional research and is
not part of the required v1 runtime.
