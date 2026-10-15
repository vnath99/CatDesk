# T-0483 R32 — Retained Sol Job empty diagnostic (source only)

## Change

Normal drain now retains the exact unnamed, kill-on-close Job handle in the bound witness and inert stopped-worker candidate. The candidate's read-only `diagnose_retained_job_empty` rechecks the Job limits, accounting and process-ID list after the earlier empty-Job observation. It rejects a late active member or failed accounting readback. The handle remains owned through the candidate rather than being closed at normal drain.

This is a diagnostic of that one Job. It does not prove that no worker descendants escaped the Job, that another process lacks inherited or duplicated write handles, or that ancestor paths deny write authority. The candidate has no nonce burn, output staging, repository-use, review or Wake method.

## Verification

- Focused lifecycle tests passed 31 with one ignored helper. A real late suspended member invalidates the retained empty observation; an injected accounting failure also refuses, and later successful readback can recover the diagnostic. Strict workspace Clippy, formatting and scoped diff checks passed.
- The isolated root shell/cwd test and remaining all-features root workspace Rust suite passed locally, including nested Wake. Strict root all-targets/all-features Clippy, root/Wake formatting, root all-bin build and diff checks passed.

Source commit `df5b5ec12251c04c0437f56be872f8a6fc1c7b940` was pushed. Immediate read-only GitHub readback found zero workflow runs and zero check-runs for that exact SHA. No remote CI acceptance is claimed.

## Remaining gate

The fixed `CatDeskSolWorker` account is absent, so no positive distinct-user restricted-token run occurred. Creation of that account, a trusted fixed-account primary-token launch, complete lost-write proof, durable burn-to-stage handoff, real Sol/medium deliverable, natural `review_ready` and exact Wake receipt remain open. The stopped candidate remains inert.
