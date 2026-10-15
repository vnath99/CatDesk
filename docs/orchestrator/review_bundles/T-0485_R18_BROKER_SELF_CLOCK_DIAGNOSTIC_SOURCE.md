# T-0485 R18 — fixed broker self-clock diagnostic (source only)

## Scope

`wake/src/install_broker_current_observer.rs` adds a read-only `BrokerSelfClockPin`. Its constructor borrows the retained fixed protected-root guard and `BrokerLedgerGuard` global lock. It accepts no caller-selected PID, executable path, clock or observation. The fixed SCM service must report RUNNING, own-process and this exact process PID. An opened process handle is checked for PID, creation time, liveness, fixed service token and compiled image path before and after sampling. The fixed protected root, policy root and ledger root must have the same physical identity. A retained fixed image handle is compared with the current preflight image identity, bytes and protected security.

The ledger supplies a rollback floor only after scanning complete chained history under the lock. Gaps, incomplete predecessors, repeated approvals and backward accepted times fail closed. The observer converts `GetSystemTimePreciseAsFileTime` with checked bounds, rejects time before process creation or the protected floor, and rejects backward samples within one retained pin. Its copied readback is labeled `READ_ONLY_SELF_CLOCK_PIN_NO_INSTALL_AUTHORITY`.

## Evidence and limits

- Focused observer tests: 3 passed, including SCM mismatch, real process handle exit/reuse checks and time bounds. Focused protected-ledger clock-floor tests: 2 passed. Independent read-only review found no concrete fail-open defect. Final combined gates passed: root all-features Rust workspace, independent Wake all-features Rust workspace (166 library tests plus bins/integrations), full Python `pytest.exe -q --tb=short`, strict root/Wake all-target/all-feature Clippy, both format checks, root all-bin build and diff check.
- The fixed broker service and protected namespace are not provisioned on this host, so the positive production self-pin path cannot be exercised here. This source does not bootstrap a service, grant an approval lease, call `accept_with_observation`, change any install pointer or bypass `WAKE_INSTALL_BROKER_UNPROVISIONED`.
- The pinned current image path and file bytes do not prove the file object that was mapped when the service started. Windows UTC is a wall clock, not an independent rollback-proof time source; an empty ledger has no initial trusted time or legacy generation anchor. The dev.84 owner/store split, signed service and policy bootstrap, independent exact-commit CI, protected transaction/reader cutover and a new Chat53 canary remain open.

## Next gate

Reconcile the old dev.84 physical owner and stores without guessing a root/PID, then establish signed service bootstrap and a trusted loaded-image/time source before considering a broker-owned install observation. Keep the diagnostic readback separate from installation authority.
