# T-0485 R29 — Durable publication-attempt marker (source only)

## Change

The dormant protected transaction writes a separate canonical, create-once publication-attempt record after a fully bound STAGED record and before any future package rename. The marker binds the signed approval, protected root, STAGED digest, and pinned staging, candidate, and packages-parent identities. The record and parent directory are flushed and read back. A restarted transaction that sees this marker returns sticky `WAKE_BROKER_PUBLICATION_ATTEMPT_ATTENTION`; it never retries publication. Bound PUBLISHED-or-later replay also requires the matching marker. Historical unbound V1 records remain readable but cannot authorize a future publisher.

This source does not rename a package, write PUBLISHED, switch a pointer, or grant install authority. Both ordinary installer holds and the fixed broker service refusal remain in place.

## Verification

- Full independent Wake suite passed after the final historical-replay correction: 209 library tests passed, one ignored, plus enabled binary/integration tests with two further ignored cases. Strict Wake checks and independent read-only review found no blocker.
- Fixtures cover marker ordering, canonical replay, a torn or colliding marker, parent-flush failure, restart attention, and missing or mismatched marker on bound PUBLISHED history. They do not prove sudden power-loss durability on the protected deployment filesystem.

Source commit `c8e9fa029ff5935d8a7b03c7d5535a240f296408` was pushed. Exact-SHA GitHub workflow and check-run readback found zero runs. The latest visible successful push run still targets older `4e9a2e2`; remote CI acceptance is absent.

## Remaining gate

A retained-handle, no-replace package rename, dual-parent flush, post-rename exact-object/ACL proof and typed PUBLISHED receipt still need production implementation. First-generation dev.84 owner/store reconciliation, signed/provisioned broker, exact-current three-job CI, protected pointer/owner/reader cutover, dev.85 activation and fresh Chat53 receipt remain open. The prior CLI HTTP 422 and missing `workflow_dispatch` trigger are distinct observations; no new dispatch or settings change was attempted.
