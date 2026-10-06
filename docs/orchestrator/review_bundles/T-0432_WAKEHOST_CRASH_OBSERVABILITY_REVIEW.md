# T-0432 - WakeHost crash observability review

## Classification

`CRASH_CAUSE_UNOBSERVABLE_AND_NO_CONTINUOUS_SUPERVISOR`

This is a source-only review. No Wake process, Store record, target, package,
or recovery state was modified.

## Current execution and ownership path

`runtime::start_installed` validates the installed host and browser artifacts
against `current.json` and `manifest.json`, then calls `start_executable`.
That function checks the host singleton lease, writes desired `RUNNING`, starts
the exact installed `CatDeskWakeHost.exe --host`, and waits up to five seconds
only for `status.json` to report `RUNNING`.

The long-lived `runtime::run` process obtains `host.lock`, refreshes status on
each loop, and returns an error for malformed state, activation failure, Store
failure, adapter failure that reaches the outer loop, or other propagated
errors. The host CLI main converts an error to a JSON line and exits with code
1.

`scripts/start-catdesk-stack.ps1` has a `Start-CatDeskWakeHost` recovery hook.
It validates the independent-owner selector, current pointer, and exact host
hash before invoking the installed host `start` command. This is an
on-demand stack-recovery action, not a resident host supervisor. There is no
source-local loop that observes a dead WakeHost and restarts it continuously.

## Why a crash cause is not currently recoverable

The normal detached child launch explicitly uses:

```text
stdin  = null
stdout = null
stderr = null
```

Thus the host CLI's JSON error line is discarded. Rust panic diagnostics also
go to discarded stderr. `status.json` contains live intent/attempt fields but
no durable host-exit sequence, exit code, failure class, or bounded crash
journal. After a dead host releases `host.lock`, a fresh `status()` read may
report STOPPED; it cannot reconstruct the cause of death. While a hung host
still owns the lock, a stale `updatedUtc` is surfaced as
`HOST_HEARTBEAT_STALE`, but no restart is performed.

The existing adapter-specific `logs/adapter.log` does not provide WakeHost
process failure provenance. It cannot explain errors before adapter spawn,
host-loop errors, process termination, or a Rust panic.

## Exactly-once restart assessment

### Claimed

`Store::claim` explicitly permits recovery only when the prior delivery is
still `Claimed` and its event/target binding is exact. The normal transition
persists `Claimed -> Submitting` before the adapter receives its submit request.
Accordingly a recovered `Claimed` delivery remains within the existing
pre-submit retry boundary. A restart must revalidate the exact current target
and must not create a new event or alter an existing receipt.

This is not permission to assume a browser write happened: if a host dies in an
ambiguous local draft state before the durable `Submitting` transition, the
existing adapter's pre-write checks remain required. The durable store alone
only proves that no submit boundary was recorded.

### Submitting

`Submitting` is deliberately non-replayable. `reconcile_submitting_sent` can
only advance it to `Sent` after an exact `EXACT_USER_MESSAGE_APPENDED` receipt
matches the stored event, generation, target digest, and message digest.
`note_submitting_attention` retains an uncertainty reason without downgrading
or rearming the record. A restarted host may issue only the existing
browser-only `reconcile` request; it must never type, press Enter, click USER
Send, create a replacement event, retarget, or call `retry_pre_submit`.

Current reconciliation is bounded to four in-memory attempts with delays
0/30/120/300 seconds and a 30-minute event-age window. The attempt map is
process-local, so a host crash/restart resets its numeric counter. The age
window and immutable phase still prevent USER replay, but a durable counter or
restart epoch is needed before claiming a crash-loop-safe reconciliation
budget.

## Narrow recommended design

1. Add a bounded, atomic, no-follow host-failure record under the existing
   Wake Store root. Persist only fixed fields: schema version, monotonically
   bounded sequence, UTC, PID, current desired state, fixed exit class, and
   optionally fixed delivery phase/event ID. Never persist raw stdout/stderr,
   URLs, page text, environment, credentials, or arbitrary error text.
2. Have the host main record a fixed redacted `RUN_RETURNED_ERROR` class before
   it emits its existing CLI error and exits. Install a panic hook only to
   record `HOST_PANIC`; it must not serialize the panic payload or backtrace.
   Early failures before a safe Store root exists remain `HOST_EARLY_EXIT`
   evidence for a supervisor, not a reason to widen paths.
3. Assign one existing, explicit supervisor owner before adding automatic
   restarts. It should require: desired RUNNING; a validated installed pointer
   and all artifact hashes; no held host lease; a PID/heartbeat consistency
   check; and no active install handoff. It must not be a generic daemon reload
   or a second host-launch path.
4. Persist a small restart ledger with a fixed maximum (for example, three
   restarts in fifteen minutes). On exhaustion, write a fixed
   `HOST_CRASH_LOOP_ATTENTION` state and require operator/review action. Do not
   silently loop or reset this budget because a process restarted.
5. Persist reconciliation-attempt count or equivalent restart-stable evidence
   with the `Submitting` record. Retain the current 30-minute window and fixed
   delay schedule. A terminal/expired ambiguous record remains forensic and
   non-replayable.

## Required deterministic tests

- A fixed `runtime::run` failure writes one redacted bounded failure record;
  raw error text and adapter stderr never appear in Store state.
- A simulated panic writes only `HOST_PANIC`; an early bootstrap failure is
  classified without unsafe path discovery.
- The designated supervisor starts exactly one valid installed host after a
  dead-owner/lease-release observation, never when PAUSED or STOPPED, never
  with a live lease, invalid pointer, hash drift, or install handoff.
- Restart budget survives process recreation and reaches crash-loop attention
  without another launch.
- Crash before `BEFORE_SUBMIT` retains/reuses only the exact `Claimed` event;
  crash after durable `Submitting` sends only reconciliation traffic and never
  USER typing/submission.
- Reconciliation budget and delay survive host recreation; exact receipt moves
  `Submitting -> Sent`, while ambiguous, stale-generation, and expired cases
  stay quarantined.
- Status reports a durable last failure and correctly distinguishes a released
  dead lease from a held stale heartbeat.

## Boundaries

The recommended work must preserve the installed-artifact verification,
singleton lease, target generation/digest binding, exact receipt requirement,
historical SUBMITTING isolation, and external Secure MCP ownership. No live
restart, browser operation, target mutation, installer activation, recovery
action, Git publication, or external-project action was performed for this
review.
