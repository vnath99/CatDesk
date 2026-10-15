# T-0485 R25 — Canonical protected pointer plan (source only)

## Change

The protected reader now exposes `plan_protected_pointer_from_stage`, which constructs canonical `current.json` bytes and their SHA-256 from a `VerifiedApprovalV1` and a supplied `StageBindingV1`. The plan uses the reader's pointer schema and byte cap, validates the manifest and stage identities, rejects object collisions with the protected root or predecessor pointer, and parses its own bytes through the canonical reader before returning them. The fixture confirms the planned bytes match an accepted physical pointer without changing the protected directory or pointer.

This is a byte plan, not a broker transaction. `VerifiedApprovalV1` by itself does not establish that the approval key came from protected broker policy, and the supplied stage IDs do not prove publication in `packages`. The result carries no pinned handle, ledger lease, writer, INTENT or install authority.

## Verification

- Focused protected-reader tests passed 13. Full independent Wake passed 199 library tests with one ignored, plus binaries and integrations. Strict Wake all-targets/all-features Clippy, formatting and diff checks passed. Independent read-only source review found no concrete blocker.
- The new fixtures compare exact canonical bytes and SHA-256 with the protected reader, check schema and byte cap, confirm no filesystem mutation, and reject invalid or colliding stage IDs and a signed name incompatible with the reader.
- The isolated root shell/cwd test and remaining all-features root workspace Rust suite passed locally, including nested Wake. Strict root all-targets/all-features Clippy, root/Wake formatting, root all-bin build and diff checks passed.

Source commit `007e6001c944df320793a6339dabceb799773d840` was pushed. Immediate read-only GitHub readback found zero workflow runs and zero check-runs for that exact SHA. No remote CI acceptance is claimed.

## Remaining gate

The first-generation dev.84 connected owner/store anchor, current live old-owner proof, signed and provisioned broker, protected policy/namespace and trustworthy image/time, broker-owned package/pointer/owner transaction, protected launch-reader cutover and exact-current-source three-job Windows CI remain open. The fixed service refuses before INTENT and ordinary install mutators return `WAKE_INSTALL_BROKER_UNPROVISIONED`; no protected dev.85 install or fresh Chat53 receipt occurred. The prior GitHub CLI HTTP 422 text and missing `workflow_dispatch` trigger remain distinct. No new dispatch or Actions setting change was attempted.
