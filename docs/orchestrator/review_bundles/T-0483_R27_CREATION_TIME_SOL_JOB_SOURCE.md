# T-0483 R27 — Creation-time Sol Job assignment (source only)

## Change

The private prepared Sol launch now uses `CreateProcessW` with `PROC_THREAD_ATTRIBUTE_JOB_LIST`, so its suspended child enters the retained unnamed kill-on-close Job during process creation. It passes the exact pinned executable as `lpApplicationName`, a writable explicit command line, a validated explicit environment, a fixed prepared working directory, and `bInheritHandles=FALSE`. It verifies exact Job membership before resume. The older `Command` plus post-create assignment route remains private to same-user diagnostic tests and cannot yield the bound fixed-worker witness.

The owner retains native process and Job handles through termination/drain. A failed process creation can return a clean refusal only after an empty-Job readback; uncertain cleanup returns an attention owner for retry. Successful normal completion requires zero exit and a drained Job without forcing termination. Aborts produce no normal witness.

## Verification

- Focused lifecycle tests: 27 passed, one ignored helper. They cover creation-time Job membership before resume, descendant membership, no inheritance of another marked-inheritable host handle, normal zero-exit drain, failed creation and retained attention on unavailable Job readback.
- A real native child argv round trip covers a path containing spaces and trailing backslashes and an argument containing embedded quotes.
- The repository-wide all-features Rust suite passed, including the isolated shell/cwd test. Strict workspace all-targets/all-features Clippy, formatting, all-binary build, and diff checks passed locally.

## Remaining gate

This is still a same-user private source path. It grants no fixed-account token, repository use, nonce burn, stage, review event, or Wake authority. A trusted host must supply policy-pinned launch inputs and a distinct restricted `CatDeskSolWorker` process, with durable attention before spawn. The drained Job proves only Job members exited; it does not exclude outside-Job descendants or duplicated writable handles. Ancestor and ledger ACL exclusion, power-loss-safe burn-to-stage recovery, a genuine Sol/medium deliverable and natural review-ready Wake remain unproven.
