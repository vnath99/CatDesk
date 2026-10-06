# T-0034-R1 — post-provider accounting review bundle

## Scope and diagnosis

The durable execution ledger already captured the Codex app-server telemetry
available before a task began and could calculate only matching rate-limit
percentage deltas.  Ordinary successful autonomous turns did not invoke the
existing read-only `account/rateLimits/read` refresh after verification, so
`postCodexUsageSnapshot` remained null and `usageDelta` remained
`UNKNOWN_NOT_CAPTURED`.

This repair changes only the autonomous runtime/accounting path.  It does not
change provider selection, Codex/Qwen routing, account state, credit state,
browser wake, targets, tunnel ownership, or any live host state.

## Post-turn lifecycle

| Boundary | Durable behavior |
| --- | --- |
| Task start | First app-server telemetry becomes the pre-turn snapshot. |
| Retry or routing notification | Cannot become a post-turn snapshot. |
| Successful verified Codex turn | Runtime performs one supported read-only `account/rateLimits/read` refresh, with exact canonical-thread metadata validation. |
| Post snapshot | `record_post_codex_observability` accepts it only for the latest Codex record with a pre snapshot and no prior post snapshot. |
| Restart after verification before capture | A terminal `COMPLETED_VERIFIED` tick with a durable pending post snapshot reuses the same session and attempts the read; it does not launch a provider turn. |
| Replay/restart after durable post | No app-server probe or replacement of the original post evidence. |
| Missing thread/read authority or unavailable/non-comparable data | No estimated value; existing `UNKNOWN_NOT_CAPTURED` is retained. |

The post refresh is intentionally outside provider routing: failures are
non-authoritative accounting absence, not a reason to alter a completed
controller outcome or select a different provider.

## Comparable-delta rules

`AVAILABLE` means only that a named rate-limit window with the same reset time
has numeric before/after percentages and that the post telemetry observation is
newer.  Credit usage remains `UNKNOWN_NOT_CAPTURED`; no token, price, or UI
value is treated as credit consumption.  Equal/older observation times and a
changed reset window remain `UNKNOWN_NOT_CAPTURED`.

## Deterministic coverage

- Comparable post read persists one post snapshot and a bounded rate-limit
  delta using a fake app-server transport.
- Retry preflight and ordinary routing telemetry cannot masquerade as a post
  reading; replay cannot overwrite the durable post snapshot.
- Stale and changed-window post snapshots remain unknown.
- Failed/retried Codex outcomes and Qwen fallback do not request post-turn
  Codex accounting.
- Missing canonical-thread authority leaves the post absent and retry-eligible
  without mutating routing or inventing accounting evidence.

## Attributable files

- `src/delegated/autonomy_accounting.rs`
- `src/delegated/autonomy_runtime.rs`
- `docs/orchestrator/review_bundles/T-0034_R1_POST_PROVIDER_ACCOUNTING_REVIEW_BUNDLE.md`

The worktree predates this ticket and is broadly dirty.  Attribution is limited
to the post-turn accounting methods, runtime invocation/gate, their focused
tests, and this bundle; no unrelated historical diff is claimed.

## Verification and residual acceptance

Completed after the final source pass:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features --no-fail-fast` — passed; 861 main
  tests passed, with only the repository's explicitly ignored operator-local
  probes.
- focused accounting and runtime post-turn tests
- `git diff --check`

The remaining live acceptance boundary is a read-only operator observation of
one real authenticated Codex app-server post-turn response.  It is not
exercised by this source/test ticket.

Independent review is required before treating this as accepted.
