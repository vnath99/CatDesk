# T-0485 R27 — Retained moved-package pin (source only)

## Change

The locked exact-resume path can now retain the already moved signed package, its manifest, four artifacts, and both `staging` and `packages` parent objects while it rechecks ledger history. The retained read-only handles deny write/delete sharing and are compared with the STAGED physical IDs; a final revalidation reopens and checks location, bytes, ACL and identities before returning a diagnosis. Mutation attempts while the fixture pin is held fail, and test cases reject ambiguous locations, changed object/bytes and an ACL change while pinned. The pin must be dropped before any future publication mutation.

This is a read-only point-in-time proof. The `packages` parent ID is retained during the diagnosis but is not bound durably into STAGED. There is no production rename, parent flush, typed PUBLISHED writer, pointer switch, owner handoff or install authority.

## Verification

- Independent Wake review found no blocker. The full Wake suite passed after production logic: 200 library tests passed, one ignored, plus binary and integration tests. The final fixture-only ACL assertion passed its focused test; strict Wake Clippy, formatting and diff checks passed afterward.
- On the settled combined source, the isolated root shell/cwd regression passed. The remaining all-features root suite passed 1,231 main tests with 28 ignored plus binaries/integrations and nested Wake. Strict root/Wake Clippy and formatting, root all-bin build and staged diff checks passed locally.

Source commit `aac1c7e2e4e5a6e820b22098ca739f2a0a67c17b` was pushed. Immediate exact-SHA GitHub readback found zero check-runs; the newest visible workflow run still targeted older source `4e9a2e2`. No remote CI acceptance is claimed.

## Remaining gate

First-generation dev.84 connected owner/store reconciliation, signed/provisioned fixed broker, trustworthy current service image/time, exact-current three-job Windows CI, durable package/pointer/owner/reader transaction, protected dev.85 activation and fresh Chat53 receipt remain open. The service refuses before INTENT and ordinary installers remain `WAKE_INSTALL_BROKER_UNPROVISIONED`. No manual dispatch or Actions settings change was attempted; the earlier HTTP 422 text remains distinct from missing `workflow_dispatch`.
