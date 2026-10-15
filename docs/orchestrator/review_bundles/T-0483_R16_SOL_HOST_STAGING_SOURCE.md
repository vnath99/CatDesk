# T-0483 R16 — Dormant host-owned Sol output staging

**Scope:** `src/delegated/sol_worker_staging.rs`, `sol_worker_output.rs` binding and module declaration. No Codex launch, repository mutation, review event or Wake send.

## Implemented handoff

The exact output verifier now retains the host expectation inside its returned `VerifiedSolWorkerOutputV1`. Staging compares that complete task, baseline, native-turn, nonce and allowed-path binding before it creates any child. This closes a cross-task relabeling case where the same verified file bytes could otherwise be staged under another task's manifest.

The nonce verifier returns a private `BurnedSolWorkerOutputV1` only after its create-once burn marker is synced and read back. The staging API accepts only that post-burn type; the plain output verifier cannot mint a staging input. It chooses its own UUID stage name under a pinned host-supplied parent, creates the payload tree through no-follow protected-directory operations, writes synced files and a host-generated manifest, and reads back a bounded exact inventory and SHA-256 bytes. Existing stage collisions fail without overwrite. Readback rejects changed bytes/manifest, extra entries, hardlinks and reparse payload entries. Incomplete crash residue remains inert and is not automatically deleted or adopted.

## Local verification

- Focused Windows staging tests: **7 passed** (actual issue → Sol/medium native-turn bind → verify/burn → stage, exact readback after export drift, collision, tampering/extra entries, hardlink/reparse rejection, changed allowlist, internal stage ID and cross-binding refusal). The first stage test also refuses a second burn.
- Existing output verifier tests: **5 passed** after binding preservation.
- Final combined root all-features suite, strict all-target/all-feature Clippy and format checks passed. Exact-commit Windows CI remains outstanding.

## Remaining authority

The staging API assumes the caller has proved the separate worker and descendants are stopped, their token has lost write access, and the pinned staging root **and created child** exclude the worker by actual Windows ACL/token readback. The current module does not establish those facts and is not wired to the live Sol route. A crash after burn and before durable stage completion consumes the nonce without a resumable stage; parent-directory power-loss metadata durability and an atomic burn-to-stage recovery transaction remain unproven. No real Codex deliverable or natural review/Wake cycle is accepted.
