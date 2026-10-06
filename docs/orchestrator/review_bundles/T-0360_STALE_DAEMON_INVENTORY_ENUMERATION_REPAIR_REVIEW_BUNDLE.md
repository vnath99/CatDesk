# T-0360 — Stale-daemon inventory enumeration repair review bundle

Date: 2026-09-08

Status: implementation and verification complete; independent T-0319 acceptance review pending.

## Why this repair exists

Fresh independent Codex/Terra-high session `adc-t0319-r2-independent-recovery-rereview-20260908` re-reviewed the cumulative T-0319 recovery chain after T-0359. It confirmed that T-0359 correctly closes the prior restart-worker `-File` execution-identity defect, but its sanctioned `cargo test --test recovery_powershell -- --nocapture` run failed the stale-canonical-daemon candidate-enumeration fixture.

A subsequent host-side sanctioned rerun passed 3/3 without source mutation. That establishes the failure as intermittent rather than a permanently broken query shape and makes deterministic boundedness/selection behavior the repair target.

## Concrete enumeration defect

Before T-0360, `scripts/query-catdesk-windows-inventory.ps1` narrowed `Win32_Process` to `Name='catdesk.exe'` and then applied `Select-Object -First 65`. `scripts/start-catdesk-stack.ps1::Get-CatDeskDaemonProcessCandidates` applied the required `--catdesk-daemon` token check only after those rows returned to the parent.

That order bounded arbitrary same-image CatDesk processes rather than the relevant daemon-candidate set. GUI or other CatDesk process modes could therefore occupy the bounded first 65 same-image rows and make a real stale canonical daemon absent from the parent candidate set. The later exact canonical path/SHA-256/creation/process-instance checks cannot classify a row that never crossed the inventory boundary.

## T-0360 correction

The `Daemons` inventory query now uses the provider-side read-only narrowing filter:

```text
Name='catdesk.exe' AND CommandLine LIKE '%--catdesk-daemon%'
```

before `Select-Object -First 65`.

This filter is explicitly not mutation authority. It only constrains the bounded observation set to daemon-token candidates. The recovery parent still performs the exact command-line token check and retains the established authoritative checks for canonical executable path, SHA-256, creation time, pinned process instance, listener continuity/absence, and foreign/multiple-candidate ambiguity. The parent still refuses more than 64 returned daemon candidates, so the one-extra-row overload remains explicit and fail-closed.

## Regression coverage

`scripts/test-start-catdesk-stack.ps1` now requires the daemon-token WMI narrowing expression to exist and to occur before `Select-Object -First 65`. Reverting to the previous same-image-cap-first order fails deterministically with:

```text
Windows daemon inventory truncates same-image rows before daemon-token narrowing
```

The real disposable stale-canonical-daemon integration fixture remains intact and continues to require production enumeration to discover the exact temporary canonical daemon before the fixture isolates that process set for destructive recovery testing.

## Verification

Post-repair verification on 2026-09-08:

- `cargo test --test recovery_powershell -- --nocapture` — PASS, 3/3.
- Second consecutive `cargo test --test recovery_powershell -- --nocapture` — PASS, 3/3.
- `cargo fmt --all -- --check` — PASS.
- `cargo test --all-targets --all-features` — PASS, exit 0.
- `cargo build --all-targets --all-features` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS, exit 0; only the existing LF-to-CRLF working-copy warnings were emitted.

The repository remains intentionally dirty. These recovery files are part of the long-running untracked source surface relative to the historical Git HEAD, so current file contents plus bounded verification are the candidate evidence; no worktree cleanup, reset, staging, or publication was performed.

## Safety and authority preservation

T-0360 did not:

- invoke live CatDesk `recover` or stop/restart the serving daemon;
- stop, restart, replace, duplicate, or otherwise take ownership of the externally owned Secure MCP runtime/tunnel;
- invoke the browser/Selenium wake bridge;
- directly edit protected project/wake target state;
- invoke reviewed promotion, bless mutable `target/release`, or fabricate reviewed-build/promotion authority;
- mutate Scheduler/services;
- publish, commit, push, reset, or clean Git state.

## Required independent reviewer checks

A fresh genuinely separate Codex/Terra-high T-0319 reviewer must:

1. confirm T-0359 remains a correct closure of the restart-worker exact regular non-reparse identity defect;
2. inspect the T-0360 provider-side daemon-token narrowing and confirm the 65-row cap now bounds actual daemon candidates rather than unrelated same-image CatDesk modes;
3. confirm provider-side name/command-line filtering is only observation narrowing and that destructive authority remains exact canonical path/SHA-256/creation/process-instance authority in the parent;
4. confirm more than 64 relevant daemon candidates still fail closed;
5. rerun the sanctioned stale-canonical-daemon recovery fixture / `recovery_powershell` target and evaluate any failure from fresh evidence rather than prior prose;
6. return exactly one verdict: `ACCEPT`, `REJECT_WITH_CONCRETE_DEFECTS`, or `INSUFFICIENT_EVIDENCE`.

T-0319 must not be marked accepted from this implementation lineage. If the separate verdict is `ACCEPT`, synchronize T-0319/T-0359/T-0360 durable task state and proceed immediately to T-0324 reviewed deployment/source parity rather than opening another speculative recovery-hardening ticket.
