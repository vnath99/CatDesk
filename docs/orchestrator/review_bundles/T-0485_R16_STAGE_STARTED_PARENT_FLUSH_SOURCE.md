# T-0485 R16 — Write-ahead stage marker and parent flush (source only)

## Implemented

- The protected ledger adds `STAGE_STARTED` between INTENT and STAGED. The fixed stage writer writes and flushes this marker before creating `staging/candidate`. A reopened marker is sticky attention: the writer refuses another create even when the candidate entry is absent.
- `install_broker_durability.rs` reopens the pinned parent directory with the write right, verifies its physical identity and non-reparse type, calls `NtFlushBuffersFileEx` with normal flags, checks the returned status, and verifies identity again. A denied or unsupported flush stops the transaction.
- The ledger flushes its parent after INTENT, STAGE_STARTED and STAGED record creation. The stage writer flushes `staging` after candidate creation and `candidate` after syncing its five files, before protected readback and STAGED. This is still a dormant source path; there is no publication or activation caller.

## Verification and limits

- Six new fault/restart tests cover failed INTENT, marker, staging-parent, candidate-parent and STAGED-parent flushes, plus an injected abort after the first artifact. They check that no second stage create or publication is authorized. The existing phase fixtures were updated; the full Wake Rust suite passed with 157 library tests plus binary/integration tests, two existing manual diagnostics ignored. Strict Wake Clippy, formatting and diff checks passed locally.
- `NtFlushBuffersFileEx` succeeded for test fixture directory handles on this Windows filesystem. The exact protected deployment filesystem, storage cache behavior and sudden power loss have not been tested. Flush completion and fixture restarts alone do not prove power-cut durability.
- Any prior seven-phase ledger would fail closed under this eight-phase ordering. No production protected broker ledger is provisioned; a first-generation dev.84 physical reconciliation remains mandatory. The installed service, trusted observer/clock, independent exact-source CI, approved package/pointer/owner transaction and launch-reader cutover remain absent. Both production install mutator holds still return `WAKE_INSTALL_BROKER_UNPROVISIONED`.
