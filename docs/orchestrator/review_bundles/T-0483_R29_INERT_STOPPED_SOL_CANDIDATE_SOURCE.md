# T-0483 R29 — Inert stopped Sol candidate (source only)

## Change

Only consuming a normally drained, bound fixed-worker Job witness can now produce the private, non-cloneable `StoppedBoundFixedWorkerCandidateV1`. It retains the original opened process and pre-resume restricted-token binding, selected task/turn/nonce, pinned ledger/output roots, create-once launch reservation, and reviewed executable lease. It exposes no nonce burn, stage, repository-use, review or Wake method. The formerly crate-visible staging entry and readback value that required a *live* fixed worker are now module-private; that live revalidation cannot prove anything after the worker exits.

## Verification

- Focused lifecycle tests: 29 passed, one intentionally ignored helper. Focused staging tests: 13 passed. A signature test keeps the stopped candidate constructor consuming the normal-drain witness. Strict workspace Clippy, formatting and diff checks passed.
- The isolated shell/cwd test and remaining all-features workspace Rust suite passed locally after this change, including nested Wake-local tests. Strict root all-targets/all-features Clippy and all-bin build passed. Independent read-only review found no concrete blocker or route from the candidate to staging.
- Source commit `84e7d2343fe4931cfe2adbbae1d6cb558f72fa3d` was pushed. Immediate read-only GitHub readback showed zero workflow runs and zero check-runs for that exact SHA.

## Remaining gate

The fixed `CatDeskSolWorker` account is absent, so no positive distinct-user bound run or post-exit identity check occurred. The candidate is intentionally inert. A separately provisioned restricted primary token, creation-time Job launch under that user, exclusion of escaped descendants and inherited/duplicated handles, ancestor write-authority denial, durable burn-to-stage recovery, real Sol/medium deliverable, natural `review_ready` and exact Wake receipt remain required. Local source tests are not live acceptance.
