# T-0483 R31 — Stopped Sol host-context diagnostic (source only)

## Change

The inert stopped-worker candidate can now diagnose its retained host context after worker output appears. It rechecks the selected nonce and exact create-once launch record, the pinned ledger/output/executable-parent directory identities, and the originally retained executable native path. Through the pinned parent, it reopens the executable component and compares object identity, length and SHA-256 with the retained reviewed artifact. The post-run check permits a nonempty output directory but neither reads nor attests its contents. It returns only `Result<(), String>` and grants no launch, nonce burn, staging, repository use, review or Wake authority.

## Verification

- Focused lifecycle tests passed 30 with one ignored helper; launch-journal tests passed six. The new fixture accepts output created after the reservation and rejects changed output or executable parents, component/object/digest, original native-path snapshot, selected nonce and launch record. Strict workspace Clippy, formatting and scoped diff checks passed.
- The isolated root shell/cwd test and remaining all-features workspace Rust suite passed locally: main binary 1,228 passed/28 ignored, with integrations and nested Wake tests passing. Python `pytest -q --tb=short` exited 0. Strict root all-targets/all-features Clippy, root/Wake formatting, all-bin build and diff checks passed.
- Source commit `6ba3c4eae992068c75eb03d408ed88899735dff60` was pushed. Immediate read-only GitHub readback found zero workflow runs and zero check-runs for that exact SHA; no remote CI acceptance is claimed.

## Remaining gate

The fixed `CatDeskSolWorker` account is absent, so there was no positive distinct-user restricted-token run. This diagnostic cannot prove that escaped descendants, inherited or duplicated handles, or ancestor write rights are gone. It does not attest the process's loaded image section or worker output. A trusted fixed-account primary-token launch, complete lost-write proof, durable burn-to-stage recovery, real Sol/medium deliverable, natural `review_ready` and exact Wake receipt remain open. The candidate remains inert.
