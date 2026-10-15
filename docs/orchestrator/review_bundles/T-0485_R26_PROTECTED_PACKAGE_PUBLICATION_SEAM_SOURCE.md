# T-0485 R26 — Protected package publication seam (source only)

## Change

The broker ledger's locked, read-only exact-resume diagnosis now classifies the STAGED package at its original staging slot or at the signed `packages/<packageId>` destination. The protected reader checks the exact manifest and four artifact bytes, security, stage IDs and staging-parent identity. If a filesystem rename occurs before a PUBLISHED ledger record, the destination is diagnosed as sticky attention; it is not treated as an automatically completed publication. Both locations present, neither present, an extra entry, a changed object ID, ACL or byte hash fail closed. The diagnosis does not mutate the ledger or pointer or authorize a new INTENT.

The integrated fixture exercises rename/crash boundaries and verifies that diagnosis leaves ledger and pointer bytes unchanged. Its directory rename and exact-file cleanup paths check the resolved parents and target containment before acting in the temporary fixture. This is test scaffolding, not a production package writer.

## Verification

- Focused publication tests and the full independent Wake suite passed locally: 200 library tests passed, one ignored, plus binary and integration tests. Strict Wake Clippy, formatting and scoped diff checks passed. Independent source review found no blocking issue after fixture containment checks were tightened.
- The isolated root shell/cwd test and remaining all-features root Rust suite, including nested Wake, passed locally. Strict root/Wake Clippy and formatting, root all-bin build and diff checks passed on the settled source.

Source commit `aa8059c99adb5600b8d2e4af7abafcb61b5b17140` was pushed. Its immediate exact-SHA GitHub readback found zero workflow runs and zero check-runs. No remote CI acceptance is claimed.

## Remaining gate

The broker still lacks a production package-publication writer, retained rename/parent-durability proof, first-generation dev.84 owner/store anchor, signed/provisioned service and protected reader/pointer/owner cutover. The fixed service refuses before INTENT, ordinary install mutators remain `WAKE_INSTALL_BROKER_UNPROVISIONED`, and no protected dev.85 activation or new Chat53 receipt occurred. Exact-current three-job Windows CI remains required. The prior CLI HTTP 422 text and missing `workflow_dispatch` trigger are distinct; no new dispatch or settings change was attempted.
