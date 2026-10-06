# T-0432 — WakeHost crash observability and supervision

## Status

DEV69_RESILIENCE_LIVE_ACCEPTED

## Trigger

After dev.68 installation, authoritative Binagotchy status later reported
`host=STOPPED`, `pid=0` while the durable canary remained pre-submit. A stronger
older transport snapshot still showed PID 66212 RUNNING, but the fresh host-lock
based status proved the WakeHost singleton had actually exited. The exact exit
reason is unavailable.

The current `runtime::status()` reports STOPPED whenever the singleton host lock
is acquirable; it does not imply `control.json` desired state was intentionally
changed to STOPPED. Restarting through the guarded Binagotchy `start` command
successfully launched the same immutable dev.68 package as PID 61740 and preserved
the exact claimed canary with no USER receipt/replay.

## Source finding

`wake/src/runtime.rs::start_executable` launches the immutable WakeHost with:

- stdin = null
- stdout = null
- stderr = null

`CatDeskWakeHost` itself prints a structured error when `runtime::run()`
returns Err and exits non-zero. Because the launcher discards stdout/stderr, an
unexpected host exit loses the error evidence required to diagnose the crash.

No always-on CatDesk/Wake supervisor currently owns automatic restart when:

- durable desired state is RUNNING,
- the current immutable package remains valid,
- but the WakeHost singleton process exits unexpectedly.

Existing starts occur only at explicit lifecycle boundaries such as install
handoff, Binagotchy/GUI start or resume, or the dedicated restart tool.

## Exactly-once restart boundary

A future bounded supervisor may safely restart only the current immutable package
through `start_installed`; it must not select another executable or target.

Restart semantics must preserve existing durable delivery rules:

- CLAIMED with no receipt remains eligible for the same USER delivery.
- ATTENTION/CHATGPT_NOT_IDLE with no receipt may use the existing bounded
  pre-submit retry.
- SUBMITTING is never replayed; restart may only enter existing read-only
  reconciliation.
- SENT is terminal.
- target generation/digest and current immutable artifact hashes must be
  revalidated before startup.

## Narrow maturity work after T-0431 canary

1. Persist bounded WakeHost exit evidence rather than discarding stdout/stderr.
   Prefer a fixed local log/status record with size/rotation bounds and no
   credentials or browser content.
2. Add a supervisor rule: when desired=RUNNING, target configured, current
   immutable package/hash valid, and host lease is free, perform a bounded
   `start_installed` restart with cooldown/failure threshold.
3. Never restart while install handoff owns the install lease.
4. Surface last exit/restart reason and count through Binagotchy/status.
5. Add deterministic tests for claimed, attention, submitting, sent, invalid
   current pointer/hash, install-handoff, and restart-storm cases.
6. Run live crash/restart acceptance only after the current dev.68 natural
   canary is resolved.

## Canary isolation

A parallel Codex review session
`adc-t0432-wakehost-crash-review-r1-20260927` was intentionally CANCELLED at
stateVersion 6 before completion so it could not create a second generation-20
review event and contaminate the single-event T-0431 natural acceptance.

No product/runtime mutation is authorized by this ticket.
