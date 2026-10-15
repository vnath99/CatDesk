# T-0485 R24 — Pinned fresh-successor predecessor gate (source only)

## Change

Before a proposed new INTENT, the production ledger path now verifies the signed successor under its global lock against a retained, completed protected predecessor. It requires a nonlegacy `POINTER_SWITCHED` binding, compares the pinned `current.json` physical ID and canonical-byte SHA-256 with the ledger, retains the protected package pins, and compares the approval and expectation's generation, predecessor package, predecessor pointer and protected root. The existing package/manifest/four-artifact identity and hash readback stays in the same guarded check.

This correspondence is a necessary check, not installation authority. The first dev.84 migration anchor and live old-owner proof are explicitly false in the current diagnostic. Production `accept_with_observation` therefore refuses before independent CI fetch or INTENT even when the signed successor matches. The old fixture acceptance helper remains test-only. The fixed SCM service independently refuses before INTENT and both normal install mutators stay at `WAKE_INSTALL_BROKER_UNPROVISIONED`.

## Verification

- The focused broker ledger suite passed 26 tests. Full independent Wake passed 197 library tests with one ignored, plus binaries and integrations. Strict Wake Clippy, formatting and diff checks passed. Independent read-only review found no concrete blocker.
- An integrated temp-root fixture covers empty-history refusal, matching signed successor with retained pins, hard migration-anchor refusal, signed wrong predecessor, generation/package/pointer drift, unbound pointer, generation overflow, same-byte pointer replacement with a different file ID, same-length artifact drift, unchanged ledger bytes and absence of a next INTENT.
- The isolated root shell/cwd test passed. The remaining all-features root workspace suite passed locally: main binary 1,228 passed/28 ignored, with integrations and nested Wake tests passing. Python `pytest -q --tb=short` exited 0. Strict root all-targets/all-features Clippy, root/Wake formatting, root all-bin build and diff checks passed.

Source commit `9e8d1c8d5f4ade8327fdcf521181faf31b901ab30` was pushed. Immediate read-only GitHub readback found zero workflow runs and zero check-runs for that exact SHA; no remote CI acceptance is claimed.

## Remaining gate

There is no independently trusted first-generation dev.84 connected owner/store anchor or current live old-owner observation, nor historical signed-approval reverify, staging-parent proof, production package/pointer writer, owner handoff or launch-reader cutover. A signed/provisioned broker with trusted policy, image and time, exact-current-source three-job Windows CI, protected dev.85 installation and fresh Chat53 receipt remain required. No live install, signing, tunnel/browser or Actions-setting change occurred. The prior CLI HTTP 422 wording and absent `workflow_dispatch` trigger remain distinct.
