# T-0485 R1 — Wake protected-install approval verifier (source only)

**Scope:** `wake/src/install_broker_approval.rs`, Wake dependency/lockfiles and library export. The production `WAKE_INSTALL_BROKER_UNPROVISIONED` guards remain intact. This source does not install a service, select an executable, open the Wake store or publish/activate a package.

## Implemented contract

The dormant verifier accepts only a bounded canonical JSON envelope with a purpose-specific Ed25519 signature over `CatDeskWakeInstallApprovalV1\0` plus canonical payload bytes. It rejects unknown/duplicate fields, changed ordering/whitespace and oversized input. The key is supplied by a future separately provisioned broker; neither candidate bytes nor the envelope choose a trust root. A verified result has **no write capability**.

Signed fields bind an exact four-file package inventory (`CatDeskWakeHost.exe`, `CatDeskBinagotchy.exe`, `adapter.py`, `wake_bridge.py`) with lengths and SHA-256 digests, manifest digest, package ID/version, approval ID, scope, issue/expiry, install generation, source commit, reviewer, predecessor package and pointer object, protected root object, old runtime root/lock, and old host PID/creation time/executable object/digest. The CI claim names the fixed repository/branch/workflow ID/path, push event, completed successful run and attempt, matching source SHA, and the three required successful Windows job IDs. This verifier checks the signed claim; the independent offline issuer must verify actual GitHub run/job evidence before signing, and the broker must recheck live physical objects before mutation.

## Local verification

- Focused Wake approval tests: **6 passed** after final field binding. They reject wrong key, tampered payload, noncanonical/unknown/duplicate wire, wrong purpose, CI mismatch, changed artifact/manifest, wrong predecessor/root/owner/generation and expired/future approvals.
- Final full independent Wake `cargo test --locked --offline --manifest-path wake/Cargo.toml --all-features`: **exit 0** (74 library tests, bin/integration tests; two explicitly manual tests ignored). This rerun includes the final approval fields and preflight source.
- Independent read-only parser review found no concrete bypass after the CI schema patch. Strict Clippy and format checks passed for the combined source; exact-commit GitHub Windows CI remains outstanding. No live release acceptance is claimed.

## Remaining T-0485 work

The fixed signed service, protected namespace and trust-root bootstrap, operator/client token checks, pinned candidate-byte inventory and stage, broker-only replay/transaction ledger, runtime launch-reader migration, dev.84 physical-store reconciliation, owner-pinned handoff/rollback, adversarial worker-token probes, exact three-job Windows CI, approval issuance and fresh Wake receipt are all outstanding. A signer created inside CatDesk, a same-user verifier, or a candidate-host call to this module would not be a valid broker boundary.
