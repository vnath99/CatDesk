# T-0357 Acceptance-Gate Convergence Checkpoint

## Scope

Documentation/read-only acceptance-gate checkpoint only. No product source, runtime, release, wake, protected state, Scheduler, external project, or Git-publication mutation is authorized by this checkpoint.

## Fresh evidence

- CatDesk transport is `CONNECTED_VERIFIED` with local MCP `READY` on the already-running official Secure MCP runtime. CatDesk attached monitoring and did not create a duplicate runtime.
- The only delegated workspace owner remains `T-0322` in `CANCEL_REQUESTED`. Starting another local Qwen reviewer would therefore duplicate/compete for delegated workspace ownership.
- Current source already contains the fail-closed T-0156 cancellation reconciliation that can safely reap a stranded `CANCEL_REQUESTED` run when no mutation outcome is uncertain. T-0352 proved the live serving generation does not reconcile this safe historical run.
- T-0353 already established that the serving generation predates the current reviewed-build compatibility bridge: the current-source compatibility request is handled by the live daemon as legacy reload input and returns `buildPath is required`.
- A fresh read-only inspection of the serving MCP catalog in this checkpoint likewise does not expose the reviewed stable-supervisor lifecycle status/preflight surfaces that exist in the accepted source chain. This is consistent with the same serving-generation skew rather than evidence for another source-hardening ticket.

## Decision

T-0223 host-live stable-supervisor acceptance and T-0324 reviewed deployment parity are now treated as one deployment-generation bottleneck: the live control plane must first reach a reviewed generation that contains the already-accepted lifecycle/reconciliation/reviewed-build surfaces before host-live supervisor acceptance, stale-run reconciliation, or a fresh local Qwen reviewer can be meaningfully exercised.

Further recovery hardening is frozen unless a concrete functional acceptance failure identifies a new blocker. This checkpoint does not extend the T-0319 implementation-review chain: T-0319 still requires a genuinely independent final review of the cumulative recovery changes through T-0356, and the current ChatGPT lineage does not self-accept that boundary.

## Next legitimate sequence

1. When a genuinely separate Codex/Terra-high reviewer is actually available, obtain the independent T-0319 review without repeated provider probing while unavailable.
2. Use only the protected reviewed-build/promotion authority for T-0324 deployment parity; do not bless `target/release`, verification-only outputs, mutable dirty-source builds, or direct promotion scripts.
3. After a reviewed serving generation is active, verify the expected stable-supervisor lifecycle surface is present and the safe stale T-0322 `CANCEL_REQUESTED` ownership reconciles.
4. Then run the bounded Qwen 3.8 continuation canary and the separately authorized T-0223 host-live supervisor acceptance.
5. Continue T-0222/T-0139 visible GUI acceptance, T-0152 integrated sweep, and T-0155 soak in the established order.

## Preserved boundaries

No provider re-probe, manual browser wake, target/config edit, runtime reload/recovery/promotion, protected wake-state edit, Scheduler/service mutation, Secure MCP restart/duplication, external-project mutation, Git publication, or dirty-worktree cleanup occurred. The external official Secure MCP runtime remains independently owned and monitored only.