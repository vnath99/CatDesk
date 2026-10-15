# T0478 R3B4-B3B2B2F — Bounded owner attention source

Date: 2026-10-14. Scope: dormant witnessed candidate stage and owner journal. This is source-only; no installed runtime, MCP tool, WakeHost, signer, or tunnel changed.

The stage now attempts a create-only, file-synced `attention.json` in its owner-only journal when a candidate parent cannot be acquired after intent, root creation is indeterminate, root binding fails, or source staging fails after binding. The record contains only schema version, snapshot ID, intent hash, optional bound root object identity, and one fixed code. It never serializes a variable failure message or a caller-supplied path. A second writer cannot overwrite the first attention record. If recording fails, the original typed stage error retains the pinned owner handle when available and includes the recording failure; it never infers cleanup authority.

The read-only attention reader checks the journal owner/DACL and exact generated name, intent bytes, canonical bounded encoding, fixed code and binding shape. A process-crash simulation verifies attention readback, second-write refusal, and tamper rejection. A missing attention file is not proof of success: process termination can occur before a failure handler runs. A present record is forensic evidence only, not a deletion or retry token.

Local Windows focused snapshot suite: 13/13 passed. Strict workspace all-target/all-feature Clippy, formatting and diff checks passed. Full root all-features Rust suite on final source exited 0 (main binary 1,078 passed, 26 ignored; remaining binary/integration suites passed). Exact-commit independent three-job Windows CI remains unavailable.

Remaining requirements: trusted owner generation and cross-process single-writer lifecycle, bounded durable recovery/operator workflow for crash intervals with no attention record, journal retirement, power-loss durability proof, T0478-C manifest-gated MCP, T0478-D worker lifecycle, T0479, and production Recovery. GitHub Actions dispatch has recently returned HTTP 422 because Actions is disabled for this repository; no local test substitutes for exact-commit independent CI.
