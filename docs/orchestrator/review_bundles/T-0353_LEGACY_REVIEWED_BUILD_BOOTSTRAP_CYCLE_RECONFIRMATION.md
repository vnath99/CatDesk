# T-0353 — legacy reviewed-build bootstrap cycle reconfirmation

## Classification

**`TRUSTED_CANDIDATE_PREREQUISITE_MISSING / DEPLOYED_RUNTIME_TOO_OLD`**.

T-0353 is a no-source-change live-control audit. It reconfirms that the serving CatDesk generation is too old to expose the current reviewed-build compatibility bridge needed to produce and promote the source-correct runtime, while the already-known stale delegated run T-0322 remains `CANCEL_REQUESTED` and prevents a fresh local Qwen independent reviewer from owning the workspace.

## Live observations

1. CatDesk transport remained `CONNECTED_VERIFIED` with local MCP `READY`; the official externally owned Secure MCP runtime was already running and CatDesk attached monitoring without creating a duplicate.
2. The only delegated workspace residue relevant to the current gate remains `T-0322` in `CANCEL_REQUESTED`. A repeated sanctioned `delegated_run_cancel` call is idempotent and leaves it `CANCEL_REQUESTED`; no protected lock/state was deleted manually.
3. The current source contains a closed `catdesk_daemon_reload` compatibility bridge for `REVIEWED_BUILD_PREPARE`, `REVIEWED_BUILD_CONFIRM`, `REVIEWED_BUILD_RESULT`, and reviewed-promotion equivalents. A read-only live probe using the exact current-source compatibility shape `decision=REVIEWED_BUILD_RESULT` returned the legacy error `buildPath is required`. Therefore the serving generation does not implement that bridge and still interprets the request as the older raw reload API.
4. This exactly reproduces the historical T-0306 host observation for `REVIEWED_BUILD_PREPARE`, which was likewise rejected with `buildPath is required`. T-0306 also established that no `.catdesk/reviewed-build`, `.catdesk/promotion-control`, or `target/reviewed-builds` candidate chain existed and that verification-only release outputs were not acceptable bootstrap authority.
5. Current source already contains the safe T-0156 cancellation-rehydration behavior needed to reap a no-uncertain-mutation stranded `CANCEL_REQUESTED` run, but T-0352 proved that behavior is not deployed. Duplicating or manually bypassing the cancellation state would weaken the fail-closed ownership boundary.

## Dependency cycle

The remaining bootstrap cycle is now explicit:

- the old serving runtime strands T-0322 and lacks current reviewed-build/promotion compatibility;
- the stranded delegated owner blocks the local Qwen path that could provide a genuinely separate T-0319 review;
- T-0319 must not be self-accepted by the implementation lineage;
- without an eligible current independent review authority, the protected reviewed-build worker cannot mint the current candidate/attestation chain;
- without that candidate chain, the protected promotion path cannot deploy the source-correct runtime that would reconcile T-0322 and expose the newer control surfaces.

This is not a reason to use `target/release`, a manually copied executable, legacy raw reload, direct promotion script execution, active-lock deletion, or any other self-blessing substitute.

## Reconciliation with T-0301/T-0306

T-0301/T-0306 already classified the pre-T-0299 bootstrap as a trusted-candidate prerequisite problem rather than a source defect. T-0353 confirms that later recovery hardening did not accidentally create a safe legacy escape hatch. The present parity problem is broader because current source also includes the T-0324 Qwen continuation fix and T-0156 cancellation reconciliation, but the accepted authority rule is unchanged: only a genuinely reviewed candidate chain may cross into canonical execution.

No previously trusted intermediate candidate was located in the durable T-030x acceptance documentation. T-0306 explicitly records the candidate/attestation/promotion chain as absent and rejects the T-0304-R3 isolated release build as bootstrap authority.

## Verification

- Live exact compatibility read probe: `REVIEWED_BUILD_RESULT` through the serving `catdesk_daemon_reload` surface -> legacy `buildPath is required`, proving the serving generation predates the reviewed-build bridge.
- `delegated_run_status(T-0322)` -> `CANCEL_REQUESTED`, with the known Ollama continuation HTTP 500 and no new mutation evidence.
- Repeated sanctioned `delegated_run_cancel(T-0322)` -> still `CANCEL_REQUESTED`; no manual state deletion.
- `git diff --check` -> exit 0; only existing LF-to-CRLF working-copy warnings.

## Next safe action

Keep T-0324 and T-0319 open. Prefer a genuinely separate Codex/Terra-high reviewer when the provider is actually available, without repeated probes while provider configuration/exhaustion is authoritative. If a separate reviewer cannot be obtained through the current serving controller, the bootstrap remains a guarded operator/provisioning boundary; do not invent an unreviewed candidate. Continue repository-only acceptance/readiness work that does not require live runtime ownership, protected wake mutation, promotion, or self-acceptance.

No live recovery, daemon/tunnel restart or migration, reviewed build confirmation, promotion, browser wake, protected wake-state edit, Scheduler mutation, Git publication, manual lock deletion, or dirty-worktree cleanup occurred.
