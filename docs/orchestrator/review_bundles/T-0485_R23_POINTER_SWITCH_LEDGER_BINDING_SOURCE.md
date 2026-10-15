# T-0485 R23 — Read-only pointer-switch ledger binding (source only)

## Change

The broker ledger can now carry the new `current.json` physical object ID and canonical-byte SHA-256 from `POINTER_SWITCHED` through `COMPLETE`. The optional field preserves the exact canonical bytes of older V1 records, but a legacy completed history without that binding cannot authorize a later INTENT. Replay rejects an early, disappearing or changed pointer binding. Cross-generation checks compare the next approval's predecessor package and pointer with the previous completed output.

Under the global ledger lock, the completed-predecessor diagnostic retains the protected reader's pointer handle and compares its physical ID and exact-byte digest with the ledger binding, alongside the existing package/manifest/four-artifact ID checks. It returns `NO_INSTALL_AUTHORITY`. Only a test fixture can set a switched-pointer record in this increment; there is no production broker-owned pointer writer. The staging parent, first-generation dev.84 migration anchor, owner transition and runtime reader cutover remain unverified.

## Verification

- Focused broker tests passed 132 tests with one ignored. Full independent Wake tests passed 197 library tests with one ignored; binaries and integrations passed. Strict Wake all-targets/all-features Clippy, formatting and diff checks passed.
- The integrated protected-reader/ledger fixture covers restart before and after `POINTER_SWITCHED`, parent-flush failure, canonical legacy-record round-trip and later-INTENT refusal, exact pointer ID/digest readback, and a same-byte replacement file with a different physical ID. Phase drift and cross-generation predecessor pointer/package drift are rejected. The previous protected package/manifest/artifact readback checks remain in place.
- The isolated root shell/cwd test and remaining all-features workspace Rust suite passed locally, including nested Wake tests. Strict root all-targets/all-features Clippy, formatting and all-bin build passed. Independent source review found no concrete blocker; it identified and the change corrected a stale reader comment.

Source commit `c5b35648223c1bb5e838d0f28a61ef28b0a128810` was pushed. Immediate read-only GitHub readback found zero workflow runs and zero check-runs for that exact SHA. No remote CI acceptance is claimed.

## Remaining gate

The production SCM service still refuses before INTENT and normal install mutators return `WAKE_INSTALL_BROKER_UNPROVISIONED`. The signed/provisioned broker, protected policy/namespace and trusted image/time observations, live connected dev.84 owner/store reconciliation and first-generation anchor, broker-owned package/pointer/owner transaction, protected launch readers, exact-current-source three-job Windows CI and fresh Chat53 receipt remain open. Neither this fixture nor an unsigned BuildOnly candidate authorizes dev.85 activation. The prior HTTP 422 dispatch text and absent `workflow_dispatch` trigger are distinct; no new dispatch or Actions setting change was attempted.
