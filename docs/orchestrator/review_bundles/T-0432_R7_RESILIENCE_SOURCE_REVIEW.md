# T-0432 R7 — WakeHost resilience source review

## Verdict

ACCEPT CURRENT T-0432 SOURCE FOR THE NEXT IMMUTABLE WAKE PACKAGE.

R6 is valid verification evidence for the pre-review candidate, but it is
superseded by R7 because the source review subsequently added a finite recovery
threshold and deferred recovery-telemetry carry-forward. R7 is therefore the
authoritative verification gate for the current candidate.

## Verified current behavior

R7 session:
`adc-t0432-resilience-verification-r7-20260927`

State:
`COMPLETED_VERIFIED`

Verification:
- strict contract-approved Clippy: PASS;
- full root Cargo suite: PASS (991 root tests);
- the root suite includes `tests/wake_local_runtime.rs`, which executes the
  ordinary Wake-local Cargo suite offline with `feature=test-support`;
- authoritative diff/final review captured by CatDesk.

The Wake-local harness was deliberately repaired across R1-R6 without weakening
production behavior. R4's deterministic nested log proved all Wake Rust unit
tests passed and isolated two stale Python test expectations; those tests were
updated to match the already-accepted post-submit contract. R6 then passed. R7
re-ran verification after the review-driven resilience hardening.

## Accepted T-0432 design

- Installed `CatDeskWakeHost --host` uses a resilient in-process loop while
  retaining one singleton HostLease across retries.
- The public one-shot `runtime::run` remains available for existing callers
  and tests.
- Recoverable errors are narrowly allowlisted to transient state/config/queue/
  receipt/timer/readiness/review-source availability and state persistence
  failures.
- Activation, target, receipt-binding, malformed-state, installed-pointer, hash,
  and immutable-artifact authority failures remain fail-closed.
- Before any retry, durable desired state must remain RUNNING, no reviewed
  install handoff may be active, and the executing host must still match the
  exact current immutable directory, host hash, adapter hash, and bridge hash.
- Existing durable delivery semantics remain authoritative:
  CLAIMED is idempotent under the same lease; ATTENTION pre-submit may use its
  existing bounded retry; SUBMITTING remains reconciliation-only; SENT is
  terminal.
- Adapter Drop kills and waits for the Python child on unwind, preventing a
  retry from orphaning Selenium/browser children.
- Backoff is bounded at 1, 2, 4, 8, 16, 32, then 60 seconds.
- A maximum of 8 consecutive recovery attempts is allowed within the five-minute
  reset window. A ninth consecutive candidate fails closed with
  `HOST_RUNTIME_RECOVERY_EXHAUSTED`, preventing an endless 60-second retry loop.
- Recovery telemetry is backward-compatible and exposed through status/MCP/CLI:
  `hostRecoveryCount`, `lastHostError`, `lastHostErrorUtc`.
- If the triggering transient failure prevents immediate status persistence,
  the process carries the recovery increment and error evidence in memory and
  merges it into status once persistence becomes available again.

## Verification-harness lessons retained

The permanent root Wake-local harness now uses the established Wake command:
Cargo's own executable path, `wake/Cargo.toml`, `--features test-support`,
`--offline`, and an isolated target directory. It intentionally does not use
`--all-targets`, which would expand into unrelated diagnostic examples.

On nested failure only, it writes a bounded diagnostic under ignored
`target/wake-local-runtime-verification/last-failure.log`. This is diagnostic
evidence only and never runtime authority.

## Boundaries

This review does not:
- modify or replace accepted installed dev.68;
- authorize direct edits to installed `current.json` or `previous.json`;
- alter the canonical generation-20 target;
- resume T-0425;
- alter external Secure MCP ownership;
- authorize a competing external WakeHost supervisor process.

## Next boundary

Allocate dev.69, re-run immutable package build/hash verification, independently
review the exact dev.69 package, publish/activate only through the reviewed
zero-argument Wake installer transaction, then perform live T-0432 acceptance.

Live acceptance must safely produce a recoverable host-loop failure with no
ambiguous USER submission, prove the same host/singleton authority survives
in-process recovery, show recovery telemetry increment/error evidence and bounded
backoff behavior, and then successfully process a normal event. Persistent
failure must demonstrate finite exhaustion rather than infinite restart.
