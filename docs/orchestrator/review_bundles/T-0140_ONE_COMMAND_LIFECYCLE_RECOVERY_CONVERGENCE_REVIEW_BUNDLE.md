# T-0140 — One-command lifecycle/recovery convergence

## Scope and status

This is a source/test/review closure for the public `catdesk.ps1` lifecycle
facade. It does not perform host recovery, promotion, daemon reload, browser
wake, Scheduler/service mutation, or Secure MCP/tunnel mutation. Independent
review remains required; this bundle does not accept T-0223 host-live
activation or any live recovery proof.

The review input was the current milestone tracker, current plan, lifecycle
facade/engine, and the available T-0137 and T-0154 bundles. No exact
T-0121-named review-bundle artifact is present in `docs/orchestrator` at this
workspace state; the durable milestone/queue references and T-0154 recovery
evidence were used instead.

## Exact deterministic gap

T-0154 made `start` and `recover` enter the bounded trusted recovery engine
before requiring a valid canonical binary/fingerprint pair. Two deterministic
gaps remained:

1. `status` still reduced every canonical identity failure to generic
   `STATUS_UNAVAILABLE`, so it could not distinguish an exact durable LKG
   recovery path from missing or ambiguous recovery authority.
2. If an interrupted-promotion recovery attempt existed but could not prove
   completion, `Repair-CanonicalReleaseForRecovery` returned that failure
   immediately. It did not continue to the already independent exact LKG
   authority, even where that LKG could restore the canonical pair safely.

Neither gap authorizes a new recovery owner, direct state edit, or supervisor
activation. The stable-supervisor lifecycle remains the reviewed T-0223 path;
this work only converges the existing public façade's fixed-vocabulary status
and exact reviewed-pair recovery behavior.

## Changed behavior

| Surface | Before | After |
| --- | --- | --- |
| `catdesk.ps1 status` with valid identity | Bounded readiness status. | Unchanged. |
| `status` with broken identity plus exact LKG | Generic `STATUS_UNAVAILABLE`. | Read-only `RECOVERY_AVAILABLE`, with the fixed source `LAST_KNOWN_GOOD`. |
| `status` with missing/ambiguous LKG | Generic canonical-status failure. | Fixed `LKG_AUTHORITY_MISSING` or `LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED`. |
| `status` with no proved recoverability | Generic canonical-status failure. | Fixed `RECOVERY_AUTHORITY_REQUIRED`. |
| Actual recover after an unproven interrupted-promotion attempt | Could stop before LKG fallback. | Falls back only to an independently validated exact LKG pair; absent/ambiguous LKG still refuses. |

`Get-CanonicalReleaseRecoveryAssessment` is the new engine-local read-only
assessment. It reads only the bounded durable LKG evidence and checks an
optional exact requested hash. It does not run the promotion helper, start or
stop a daemon, verify or mutate the external runtime, or create state.

The façade invokes the existing engine in `-Mode recover` without `-Execute`
only after canonical identity fails. The engine returns its bounded assessment
before it loads local-MCP/tunnel configuration or reaches any lifecycle side
effect. Facade parsing accepts only the fixed state/source vocabulary and
reduces malformed, oversized, or unknown engine output to the existing redacted
failure result.

For actual recovery, an interrupted-promotion attempt remains first. If it
cannot prove completion, the code can continue only through
`Get-CatDeskLastKnownGoodRelease`; no transaction file, listener, or current
disk binary is self-blessing authority. A missing, damaged, ambiguous, or
requested-hash-mismatched LKG maps to the existing fail-closed categories.

## Regression coverage

`scripts/test-catdesk-lifecycle.ps1` now proves:

- healthy `status` is idempotent and non-mutating;
- an exact LKG and an exact interrupted-promotion result produce only bounded
  `RECOVERY_AVAILABLE` statuses;
- missing and ambiguous recovery authority produce fixed fail-closed states;
- status assessment does not invoke recovery, compile, install, or gain tunnel
  ownership; and
- existing start/recover routing, distinct-listener refusal, and redaction
  tests remain intact.

`scripts/test-start-catdesk-stack.ps1` now proves:

- the assessment leaves a deliberately mismatched canonical sidecar unchanged;
- an unproven interrupted-promotion attempt falls back to exact LKG evidence;
- repeated recovery is idempotent after restoration;
- genuine missing and constructed same-generation ambiguous LKG states refuse;
- the assessment function has no daemon launch/stop, interrupted-promotion
  execution, runtime verification, or migration authority; and
- existing same-owner dual-stack listener and distinct-owner ambiguity behavior
  remains covered.

## Verification

| Check | Result |
| --- | --- |
| `powershell -File scripts/test-start-catdesk-stack.ps1` | PASS |
| `powershell -File scripts/test-catdesk-lifecycle.ps1` | PASS |
| `cargo test --test recovery_powershell -- --nocapture` | PASS (2 tests) |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (839 tests; existing optional Python-advisor tests ignored) |
| `cargo build --all-targets --all-features` | PASS |
| `cargo check --release --bin catdesk --bin catdesk-control-plane-supervisor` | PASS |
| configured `rust_full` / project verification | Not configured in this workspace |
| `git diff --check` | PASS |

Cargo emitted the pre-existing non-fatal warning that it could not canonicalize
`<USER_PROFILE>`; it did not fail a command.

## Attribution

Task-attributable lifecycle/test paths are:

- `catdesk.ps1`
- `scripts/start-catdesk-stack.ps1`
- `scripts/test-catdesk-lifecycle.ps1`
- `scripts/test-start-catdesk-stack.ps1`
- this bundle

These lifecycle scripts were already untracked in the intentionally dirty
worktree, so Git cannot produce a tracked-file hunk isolating the new lines.
The named functions and regression assertions above are the reviewable narrow
attribution boundary. No unrelated tracked or external state was cleaned,
reset, staged, committed, published, or modified.

## Residual live/operator-only acceptance

T-0223 stable-supervisor host-live activation/continuity remains parked and
must use the separately reviewed closed lifecycle authority when a future
approved ticket permits it. T-0224 Rust wake cutover/W13 restart evidence also
remains open. This source ticket neither activates the supervisor nor changes
the external Secure MCP/tunnel, a ChatGPT target, browser state, worker
lifecycle, or wake ownership.

## Independent-review checklist

- Confirm `status` is read-only and its only new result vocabulary is bounded.
- Confirm LKG fallback happens only after the interrupted-promotion path fails
  and only with exact durable LKG validation.
- Confirm missing/ambiguous/mismatched authority cannot trigger recovery.
- Confirm no tunnel ownership, duplicate lifecycle route, or stable-supervisor
  host activation was added.
- Independently review the narrow attributable sources and final verification
  output before accepting T-0140.
