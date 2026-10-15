# T-0485 R28 — Durable packages-parent STAGED binding (source only)

## Change

New typed STAGED records carry the pre-existing protected `packages` parent's physical ID. The stage receipt pins that parent before STAGE_STARTED/candidate creation, checks its protected ACL and ID, and holds/revalidates it across the STAGED record and root flush. The locked read-only publication diagnosis compares the live parent to the durable binding and reports whether that premise matched. A same-name parent replacement fails even if package bytes are moved back. The field is optional at the end of the V1 schema so old canonical records replay unchanged; legacy records remain readable and cannot authorize a future publisher.

This source does not rename a package or write PUBLISHED. The stage-location parent readback is point-in-time; there is no pre-rename attempt marker or retained post-rename writer receipt yet.

## Verification

- Full independent Wake suite passed after the full-record legacy replay test: 204 library tests passed, one ignored, plus enabled binary/integration tests with two further ignored cases. Strict Wake all-targets/all-features Clippy, formatting and diff checks passed. Independent read-only review found no deterministic blocker.
- Tests cover exact parent ID in new STAGED history, missing parent before marker/candidate, old canonical V1 replay, object-ID aliases and same-name parent replacement during resume.

Source commit `b74ef2e00b1d21d9cd97a058af6fc94e431b5b7b` was pushed. Immediate exact-SHA GitHub readback found zero check-runs; latest visible workflow run still targeted older `4e9a2e2`. No remote CI acceptance is claimed.

## Remaining gate

A crash-safe pre-rename attempt marker, handle-sharing-compatible no-replace rename, both parent flushes and a typed PUBLISHED receipt remain to be built and tested. First-generation dev.84 owner/store reconciliation, signed/provisioned broker, exact-current three-job CI, protected pointer/owner/reader cutover, dev.85 activation and fresh Chat53 receipt remain open. The fixed service and ordinary installers still refuse before mutation. No dispatch or Actions settings change was attempted.
