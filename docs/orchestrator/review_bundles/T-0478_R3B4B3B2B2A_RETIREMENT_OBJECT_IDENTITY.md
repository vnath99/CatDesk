# T0478 R3B4-B3B2B2A — Retained object-identity retirement evidence

Date: 2026-10-12. CatDesk Chat52.

**Status: SOURCE-ONLY, NO DELETE AUTHORITY.** This increment precedes the owner-verified native cleanup needed to retire disposable Git-source fixture roots without accidental outside deletion.

## Predecessor

B3B2B1 original owner/DACL readback commit `39caacb` GitHub Windows Actions run `38194352700` failed the positive native ACL readback test because it required exact serialized SDDL. Three unrelated job-manager tests also timed out. A correction `ad7019345e8b523bdc53379e62cc0422aad4b975` compares exact owner SID and two protected full-access ACEs semantically (user and LocalSystem) rather than relying on ACE order and symbolic/numeric rights text. Local positive native ACL test, negative policy test, full main Rust suite and all-target strict Clippy passed after the correction; independent CI run `38235173396` pending final three-job acceptance at this writing.

## New B3B2B2A source-only evidence

- `src/isolated_fixture_snapshot.rs::StagedCandidateSnapshot::review_retirement_object_identities()` runs the already-reviewed non-destructive `review_retirement_inventory`: requires generated canonical OS-temp child, manifest exact file count and names, single-open no-follow unique-link reads and SHA-256/size matches.
- It then requires the exact number of *already retained* reviewed file leases to match the manifest entries. Each retained lease must have matching SHA/byte length and a unique Windows volume/file-index object identity. It captures the root's volume/file ID **from its already open pinned directory handle**, plus the internally generated snapshot UUID and each manifest leaf's retained Windows object identity.
- The returned `CandidateRetirementInventory` is immutable in-memory **diagnostic evidence only**. It holds no DELETE handles, authorizes no external paths, and is not a capability to retire data. There is no automatic cleanup, process termination, Task Scheduler adjustment, MCP route or background worker activation.
- Windows regression test validates the root/file identity evidence for exactly test-owned accepted candidate file and refuses to create this evidence if a foreign file was injected. Earlier tests reject foreign directory, missing expected file and hardlinks to an external fixture, preserving external bytes.

## Verification

Local `cargo fmt --all`, strict all-target/features `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings`, focused `cargo test --locked --offline --bin catdesk isolated_fixture_snapshot` 5/5 PASS and full `cargo test --locked --offline --bin catdesk` exit0. Independent GitHub Windows Rust, Python Wake/advisor and independent WakeHost CI required for this new source commit before acceptance.

## Next security gate

B3B2B2B must combine this evidence with independent process-token owner/DACL verification, compare fresh opened DELETE-authorized handles to the recorded volume/file IDs and SHA, and use **only the existing audited native opened-handle disposition primitives** to delete exactly those known files. It must not directly call the unrestricted recursive `reviewed_source_snapshot.rs::cleanup_staging_tree` or filesystem pathname `remove_dir_all`. Verify all files and identities before the first deletion; unexpected/foreign/reparse or changed names, missing/duplicate hardlinks, active process or lease, owner drift or failed native disposition must refuse cleanup or enter operator attention. Directory removal must require exact opened-handle emptiness and identity after all known files are gone. Never turn a generated directory name or this read-only evidence object into automatic delete authority.

Real fixture MCP read/search still operates without a runtime-witnessed source manifest; T0478-C must bind its own `StagedCandidateSnapshot` under exact auth and loopback. T0478-D still needs bounded owned child PID/port lifecycle; T0479 stable versioned worker/LKG and production supervisor Recovery remain unfinished. GitHub mirror already complete, hourly deadmen OFF, signed T0366 pending2 and externally owned Secure MCP unchanged.
