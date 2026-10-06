# T-0060F-R4 Durable Phase-1 Identity Review Bundle

## Repair

T-0060F-R3 treated restart-handoff evidence older than 900 seconds as stale.
That rejected the observed valid state where the reviewed candidate remained
the sole local CatDesk listener after Phase 1. The promotion helper now uses
process-instance continuity instead of an absolute evidence-age limit.

The successful handoff record is now schema 3 and includes the replacement
process start time, alongside the already bounded workspace path, build path,
SHA-256, replacement PID, status, port, and completion timestamp. The live
listener evidence independently reads the positively identified process start
time, resolved executable path, and measured SHA-256. No command line,
environment, credential, browser, or tunnel data is inspected.

## Fail-closed gate

Resume remains reachable only after the existing interrupted-transaction,
candidate containment/hash, and canonical binary/manifest checks. It then
requires exactly one loopback listener on the fixed local MCP port, named
`catdesk`, with the durable replacement PID and exact candidate path/hash.

The process start must be readable and valid. It must not be materially after
the recorded completion time (30-second explicit clock-skew allowance), nor
implausibly earlier than completion (180-second maximum startup-to-completion
lead). Schema-3 records additionally require the durable and live start times
to agree within five seconds. This rejects PID reuse/restart, future or skewed
timestamps, missing/malformed start evidence, and inconsistent process
identity without relying on how long the uninterrupted candidate has run.

Existing schema-2 handoff evidence from the observed R3 event remains
recoverable: its exact workspace/build/hash/PID and the independently current
process start time must still meet the bounded completion relation. Schema-1
compatibility keeps the prior location-plus-live-listener containment proof.
Unknown schemas and all malformed, mismatched, duplicated, or unproven states
return `OPERATOR_ATTENTION_PHASE1` before backup, transaction creation,
canonical mutation, or another candidate handoff.

The T-0060C-R2 transaction recovery path still runs first whenever a durable
promotion transaction exists. `tunnelAction` remains `NONE`; this helper has
no external Secure MCP, scheduler, browser, or direct process-lifecycle
ownership.

## Deterministic coverage

The temporary-workspace PowerShell fixture now proves:

- a 16-minute-old and a seven-day-old Phase-1 record resume when exact process
  continuity is present;
- the real older schema-2 record shape resumes only under the same continuity
  proof;
- reused PID/newer start, unreadable start, malformed durable start, future
  completion, implausibly old start, wrong PID/path/hash/workspace, no or duplicate listeners,
  malformed/unknown records, invalid canonical evidence, and transaction
  precedence all fail closed before canonical mutation;
- post-resume invocation is idempotently `ALREADY_CURRENT` and the helper
  still contains no tunnel operation.

## Local verification

Completed in this worker pass:

- `scripts/test-promote-reviewed-catdesk-build.ps1`
- promotion-source check confirming the obsolete 900-second gate is absent
- PowerShell parser checks for the promotion and restart scripts
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test` — 457 passed, 18 ignored, 0 failed
- `git diff --check` (passed; existing CRLF notices only)

Authoritative full dirty-tree capture and independent CatDesk verification
remain pending. No live promotion, reload, lifecycle, tunnel, scheduler,
browser, credential, or Git publication action was taken.

## Operator retry after independent review

The normal reviewed candidate command is unchanged. Only after CatDesk-host
approval, its execute form can resume the already-live Phase-1 candidate:

```powershell
.\scripts\promote-reviewed-catdesk-build.ps1 -BuildPath .\target\t0060f-r1-candidate\release\catdesk.exe -Execute
```

If the durable identity proof is not exact, it returns operator attention and
leaves canonical files unchanged.
