# T-0483 Sol worker host authority boundary — source hold

**Status:** source-only safety hold; native Sol execution and P1 acceptance remain blocked. **Date:** 2026-10-15.

## Observed topology

The autonomous runtime constructs the Codex CLI with `working_directory = contract.workspace` and `sandbox = workspace-write`. Its session state, approved contract and queue, checkpoint, completion, review binding, project routing, and diagnostics currently reside under `contract.workspace/.catdesk`. A provider process allowed to write its cwd can rewrite these host authority records. A matching native model/effort report alone cannot make such records trustworthy.

The [Codex workspace-write policy](https://github.com/openai/codex/blob/main/codex-rs/prompts/templates/permissions/sandbox_mode/workspace_write.md) permits edits in `cwd` and configured writable roots. An owner-only DACL for the same interactive user would not separate a same-user provider from CatDesk host authority.

## Implemented source guard

- `CodexCliProviderV1::spawn_turn` checks Sol/medium workspace-write against host-provided authority roots immediately before process spawn. Missing, unavailable, nested, parent, or resolved alias overlap refuses launch. The host currently supplies `workspace/.catdesk`, so a native Sol turn cannot start in this topology. Terra behavior and Qwen provider construction are unchanged.
- `autonomy_runtime::start_or_tick` checks queued and rate-limited Sol routes before app-server thread preparation or controller execution. Overlap writes an escalation, event and review inbox item before changing the session to `WAITING_FOR_CHATGPT`; a retry adopts an inbox item written just before a crash. A prior provider turn also sets the sticky profile contamination marker. Wake dispatch is attempted from that review record.
- The 30-second reviewer ticker intercepts an exact Sol authority-boundary launch error, rechecks the persisted contract and topology, and records the same attention if no provider handle is durably running.
- A disjoint path result is only a necessary topology condition. It does not attest the effective Windows sandbox, user-configured writable roots, a distinct worker identity, pinned host authority objects, or crash recovery. Moving `.catdesk` to a sibling directory alone is not promotion authority.

## Local verification

- `cargo test --locked --offline --bin catdesk delegated::codex_cli::tests::sol_workspace_write_refuses_host_authority_overlap_before_spawning --quiet` — passed.
- `cargo test --locked --offline --bin catdesk sol_host_authority --quiet` — passed.
- `cargo test --locked --offline --bin catdesk sol_authority_hold --quiet` — passed.
- `cargo test --locked --offline --bin catdesk ticker_codex_start_boundary_error_becomes_durable_sol_attention --quiet` — passed.
- `cargo check --locked --offline --all-features --quiet` — passed.
- `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` — passed.

No live Codex turn, installer, tunnel, WakeHost, or production state was changed by these tests. Independent CI and a real isolated Sol work/review/Wake cycle remain unproven.

## Remaining promotion work

Create a worker writable source/worktree that excludes all host authority, move or pin every authority record outside worker writes, verify the effective Windows sandbox and any extra writable roots with adversarial writes, and reconcile a parent-owned child after crashes. Then run an exact native Sol/medium turn, verification, immutable review binding, and Wake handoff under that boundary. Existing in-flight or historically overlapping Sol sessions cannot be accepted as evidence of this new boundary.
