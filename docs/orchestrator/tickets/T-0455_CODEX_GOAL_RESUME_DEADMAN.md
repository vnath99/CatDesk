# T-0455 — Codex Goal Resume Deadman

## Objective
Expose one closed CatDesk operation that lets a scheduled ChatGPT deadman reactivate existing persisted Codex Goals by exact display title without shell access, interactive `codex resume --all`, arbitrary prompts, raw thread IDs, caller-selected working directories, or authentication inputs.

## Current implementation
Source operation: `catdesk_codex_goal_resume`.

Input is titles only, for example:
`{"titles":["Implement guide features to working","Polish BYOVD driver pipeline"]}`.

The host reads Codex rate-limit telemetry first, discovers shared Desktop/CLI history by normalized exact title, fails closed on missing/ambiguous/busy matches, reads the persisted Goal, and mutates only an existing paused Goal with native `thread/goal/set` status `active`. Active Goals are no-ops, absent Goals return NO_GOAL, and other states are reported without mutation. At most eight unique titles are accepted.

This does not literally type `/goal resume`. It uses the native persisted-Goal RPC. Live acceptance must still prove that status activation resumes actual Goal execution in the intended sessions.

## Verification and remaining acceptance
- Native exact-title Goal RPC regression: PASS.
- Fresh `cargo test --bin catdesk`: PASS.
- Source is registered in MCP schema/dispatch.
- Current serving CatDesk does not expose the tool yet.
- Remaining: independent review; reviewed serving activation; live two-title call; prove usage-limited, ambiguity/busy, paused->active, and actual continued execution.

## Safety boundary
No arbitrary shell, executable, auth/profile, thread ID, cwd, prompt, model, sandbox, CLI argument, or slash-command text is caller controlled.
