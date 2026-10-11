# T0478 R3B4-A — Strict candidate source snapshot manifest validation

Date: 2026-10-11, CatDesk Chat52 canonical `https://chatgpt.com/c/6acba3bf-e740-8331-8896-8a25fefdbf16`.

## Purpose and trust boundary

This is only a **pure schema/byte-validation foundation** for future trusted fixture snapshots. The source code does not read Git, copy tracked objects, launch processes, select a source commit, enable MCP writes or authorize production/real-workspace access. Validation of a manifest's text and SHA digest cannot prove the claimed `sourceCommit` was reviewed. A separate trusted Git-object producer, independent GitHub ref/CI check, and in-process runtime policy are still mandatory.

## Source change

- Added `src/isolated_fixture_manifest.rs` behind a deliberately dormant `mod isolated_fixture_manifest` registration in `src/main.rs`. The `#[allow(dead_code)]` comment explicitly marks R3B4-A as an unbound preparatory module.
- Strict `serde` structs (`deny_unknown_fields`, camelCase) for versioned manifest with `schemaVersion`, `sourceRepository`, `sourceCommit`, `snapshotId`, and sorted `files[{fixtureLeaf,sourcePath,sizeBytes,sha256}]`.
- Require repository literal `https://github.com/vnath99/CatDesk.git`, syntactic 40-character lowercase hex commit, canonical UUID snapshot ID, schemaVersion1 and at least one entry.
- **Fixed reviewed initial source mapping only**: `Cargo.toml` => `Cargo.toml`, `main.rs` => `src/main.rs`. No caller-selected path or recursive traversal; rejects `config.toml`, dotfiles, absolute/traversal/backslash, reserved/unmapped names. Sorted distinct fixture leaves and source paths.
- Per-file upper bound 512 KiB, total at most 4 MiB, at most 32 entries (current fixed allowlist is even smaller); each SHA-256 lower-case hex 64 characters. `checked_add` on totals. Raw JSON cap 16 KiB, exact serde canonical roundtrip rejects whitespace, alternate key order, duplicate keys and ambiguous encodings.
- Pure `verify_file_bytes` checks exact byte length and SHA256 against expected bytes in memory; this does **not** open arbitrary paths and is not called yet by the MCP router.

## Negative tests and local verification

Six focused tests: valid canonical input and matching bytes, same-length tamper rejection, wrong length, noncanonical/oversized/malformed JSON, unknown/missing/extra fields, invalid/uppercase/empty/nonhex commit and wrong repo, invalid/noncanonical UUID, absolute/traversal/dotfile/config/unmapped source mappings, unsorted/duplicate entries, invalid length and uppercase/malformed hashes, duplicate JSON keys. Tests explicitly do not claim a cryptographic review of the Git commit.

Windows workspace outputs: `cargo fmt --check` PASS; `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings` PASS; `cargo test --locked --offline --bin catdesk isolated_fixture_manifest` **6 passed, 0 failed**; `cargo test --locked --offline --test isolated_dev_fixture` **1 passed** real-process test; full `cargo test --locked --offline --bin catdesk` **exit0**. Final GitHub Windows Actions three-job result is pending until commit and CI proof.

## Pending R3B4-B/C/D

R3B4-B: exact immutable Git blob source builder with independently witnessed expected commit/branch/CI, fixed source allowlist, owned temporary root and verified byte copies. R3B4-C: bind trusted manifest in isolated server state **without accepting client-provided manifest as authority**, verify each read/search response against manifest from its single no-follow pinned-handle bytes and refuse new/tampered data. R3B4-D: source-snapshot lifecycle/provenance tests and PID/port ownership, bounded kill/restart/recovery. Do not introduce general source read/write, shell tools, or an LLM agent until independent security gates.

Production stable supervisor root/startup policy remains incomplete, signed installed T0215 epoch1 and pending signed T0366 epoch2 unchanged, official external Secure MCP runtime owned separately. Preserve seven unrelated historical untracked files. No deployment/reload/production Wake action.
