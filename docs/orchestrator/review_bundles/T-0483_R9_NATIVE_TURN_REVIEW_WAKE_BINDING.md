# T-0483 R9 — Native Codex turn to independent Wake review binding

Date: 2026-10-15. Status: source review; P1 live acceptance held.

## Reviewed source increment

- `src/delegated/codex_cli.rs` reads the bounded Codex CLI 0.160.0 native rollout for the exact terminal turn. It requires matching thread/turn identity and the actual model/effort in `turn_context`; a thread's mutable app-server configuration is not accepted as turn evidence.
- `src/delegated/autonomy_accounting.rs`, `autonomy_state.rs`, and `autonomous_controller.rs` retain the exact profile in accounting and passed verification. Missing, downgraded, interrupted, or mismatched Sol turns set a sticky hold; a later matching turn cannot clear it. Session and accounting state mutations now serialize across writers.
- A completed Sol review binds the approved task, native turn, verification checkpoint, completion artifact, review ID and time, and committed source snapshot. Direct ChatGPT finalization under a Sol contract is rejected. Crash replay reuses the same bound review ID.
- `wake/src/review_binding.rs` independently checks that binding and snapshot before natural `review_ready` discovery. A cross-crate test sends a CatDesk-produced review through the Wake validator. Historical Terra and pre-contract inboxes retain compatibility, subject to their existing trust boundary.

## Verification and limits

The local all-features Rust suite, strict Clippy, formatting and Python suite passed during this source session; focused negative tests cover missing/downgraded profiles, direct Sol completion, interrupted output, thread mismatch, replay and concurrent state writes. Rerun all suites on the final scoped commit and verify its independent Windows CI before promotion.

The CLI rollout is host-local metadata, not a model-service signature. Current Codex `WorkspaceWrite` includes `.catdesk/autonomy` and other host authority. A separate fail-closed guard therefore holds Sol launch with durable attention until an OS-enforced worker/host write boundary is implemented and tested. No real Sol deliverable, natural review Wake, ChatGPT review, or next Codex task has been demonstrated. The installed WakeHost remains dev.84; no live installation or old-event replay occurred. GitHub Actions has returned HTTP 422 for new branch runs.

## Next acceptance evidence

1. Separate worker writes from CatDesk state, review records, rollout evidence and Wake authority with an effective OS identity boundary; test attempted authority writes from the actual worker token.
2. Execute a bounded Sol/medium task with exact native turn readback, concrete changed files and tests, then independently review its exact committed source snapshot.
3. Restore exact-commit three-job Windows CI, complete reviewed WakeHost installation, and deliver one new naturally produced `review_ready` to the correct Chat53 generation with `EXACT_USER_MESSAGE_APPENDED`, `SENT` and timer `COMPLETE` exactly once.
