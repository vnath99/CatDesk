# T-0429 — Direct ChatGPT pause/resume continuity review bundle

## Review target

Review only the attributable T-0429 change in `src/delegated/autonomy_supervisor.rs` plus its regression coverage. The surrounding worktree is intentionally dirty from prior accepted CatDesk/Wake work and is not attributable to this ticket.

## Problem

A direct-ChatGPT-owned session can be intentionally paused while its current queue task remains `WORKER_RUNNING`, with `provider_route = WAITING_FOR_CHATGPT`, a positive provider turn count, and no provider thread/handle. Generic resume previously forced this shape to `QUEUED` and ran provider restart reconciliation, stranding the direct-owned task in `PENDING_RECOVERY`.

## Narrow repair

When and only when all durable direct-ownership markers match, resume restores `WAITING_FOR_CHATGPT` without provider restart reconciliation or queue ownership mutation. If the shape resembles paused direct ownership but the queue cannot prove the exact current task remains `WORKER_RUNNING`, resume fails closed before mutation. All other resume cases keep the existing QUEUED/reconciliation path.

## Required reviewer checks

- No provider is launched or provider ownership synthesized.
- No task-output baseline is recaptured.
- No queue ownership is rewritten.
- Missing/inconsistent direct queue evidence refuses before state mutation.
- Interrupted provider-owned work still takes restart reconciliation.
- Direct finalization remains callable after a valid resume.
- No Wake, target authority, Secure MCP ownership, recovery/LKG, supervisor activation, or Git publication behavior is changed.

## Verification evidence

Current-turn evidence before review:
- focused direct resume regression: PASS;
- interrupted-provider reconciliation regression: PASS;
- autonomy supervisor module: 28/28 PASS before the added refusal regression;
- strict `cargo clippy --bin catdesk -- -D warnings`: PASS;
- broad `cargo test --bin catdesk`: 968 passed, 0 failed, 22 ignored;
- `cargo build`: PASS.

The connector rejected/failed to return later repeated command invocations after the refusal regression, so do not manufacture a post-addition global PASS. Source inspection confirms the refusal is before mutation. Whole-worktree `cargo fmt --check` is independently blocked by pre-existing formatting drift in `src/reviewed_build.rs` around the `legacy_stdio_definitions.lib` test; that file is outside T-0429.

## Live boundary after acceptance

Do not touch T-0425 until this exact repair is independently accepted and serving/current parity is established through the reviewed build path. Then resume the SAME session and require PAUSED -> WAITING_FOR_CHATGPT with no restart reconciliation.
