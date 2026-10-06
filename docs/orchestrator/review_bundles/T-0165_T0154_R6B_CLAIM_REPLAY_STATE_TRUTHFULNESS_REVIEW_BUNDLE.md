# T-0165 / T-0154-R6B — Claim Replay State Truthfulness

R6A's existing-claim branch returned `Ok(authorization)` without spawning.
The MCP facade treated every `Ok` as `PROMOTION_SCHEDULED`, falsely reporting
success after a crash or spawn failure between durable claim creation and worker
ownership.

`confirm_reviewed_promotion` now returns bounded confirmation outcomes:
`NEWLY_SCHEDULED` only after the create-new claim owner successfully spawns;
`CLAIMED_PENDING_UNPROVEN` for an exact replay claim with no owner/result;
`WORKER_OWNED_PENDING` only for an exact durable owner marker; and exact bound
terminal `PROMOTION_COMPLETED` or `PROMOTION_FAILED_OR_AMBIGUOUS` result state.
Replay never spawns. Result and owner files are bounded regular files and must
bind the existing authorization generation, candidate hash, and transaction.

The supervisor maps only `NEWLY_SCHEDULED` to `PROMOTION_SCHEDULED`; pending,
in-progress and terminal states remain distinct and redacted. R6A's claim,
one-shot worker-owner marker, preassigned transaction, R6 review revalidation,
direct-script authorization gate, LKG ordering, and tunnelAction NONE remain
unchanged. Source-to-candidate provenance is still unsolved and blocked on
T-0163.

Changed files: `src/daemon_reload.rs`, `src/delegated/autonomy_supervisor.rs`,
and this bundle. `cargo fmt --check` and `cargo check` passed. No live
promotion, reload, recovery, tunnel, browser, Scheduler, external-project, or
Git-publication action occurred.
