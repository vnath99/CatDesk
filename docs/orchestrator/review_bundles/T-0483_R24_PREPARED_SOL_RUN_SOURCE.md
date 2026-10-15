# T-0483 R24 — Prepared Sol run binding (source only)

## Change

The dormant Windows lifecycle now has a private, non-cloneable `PreparedSolRunV1`. It owns the host-supplied task/turn/contract/baseline/path authority, one create-once selected nonce, and the same pinned ledger and output-directory guards. Preparation rejects an unsafe or case-colliding path allowlist and a nonempty output root before selecting a nonce. It reads back the selected active/issue journal pair and checks the same pinned output root again after selection. The private fixed-worker transition consumes this value while the Job child is still suspended, rechecks the selected journal pair and empty output root immediately before resume, and carries it through the bound owner into the drained-Job witness. A same-user diagnostic `Command` child still has no route to that witness.

The native Codex turn ID is deliberately absent from preparation. It can only be bound later from trusted host-side telemetry through the existing create-once native-turn journal. This change adds no consumer to nonce burn, staging, repository use, runtime, review, or Wake.

## Verification

- `cargo test --locked --offline sol_worker_lifecycle -- --nocapture`: 11 passed; one spawned-helper test ignored in the normal runner.
- `cargo test --locked --offline sol_worker_output -- --nocapture`: 5 passed.
- `cargo clippy --locked --offline --all-targets --all-features --workspace -- -D warnings`: passed.
- The two edited Rust files were formatted with `rustfmt --edition 2024`; scoped diff check passed.

The new tests cover one selected nonce and no reissue, preexisting output and unsafe/case-colliding allowlists rejected before nonce issue, added output and altered ledger records rejected on the preparation recheck, ledger/output same-directory alias rejected after selection with the turn left in attention, and the continued failure of same-user diagnostic process binding even with a prepared context. These tests exercise temporary same-user directories. They do not validate a real fixed-account run.

## Boundary

This is a necessary host-held association, not a worker or repository authorization. The constructor's authority is host supplied; no live trusted launcher supplies it yet. The private bind call accepts the prepared context and process binding separately; a future launcher must prove that the intended child, executable, task, nonce and output root belong to one run. `ProtectedDirectoryGuard` pins directory identity but does not prove that the ledger ACL excludes the worker, that output/ledger ancestors resist write or rename, or that another process cannot mutate either directory. The observed empty-root checks cannot detect bytes written and removed between checks. The nonce journal is create-once, but parent-directory metadata durability across power loss remains unproven; a failure after active selection leaves the turn in attention and cannot silently reissue it.

The drained witness still covers only the original process and its retained Job members. It does not exclude escaped descendants or inherited/duplicated writable handles. No fixed `CatDeskSolWorker` account is present on this host, so the fixed-account bind and bound drain could not be run positively. The current stage readback requires a live R20 process binding and cannot be used unchanged after drain. A future stopped-worker handoff must separately prove lost write authority and derive the exact expected native turn from trusted host state before consuming this context at burn/readback. No live Sol→review→Wake acceptance is claimed.
