# T-0478 R3B4 — Trusted source snapshot and immutable fixture read contract

**Status: DESIGN ONLY — NOT IMPLEMENTED OR ACCEPTED.** 2026-10-11, CatDesk Chat52.

## Why this exists

R3B2 eliminated the disposable fixture's `config.toml` parse/rewrite race and R3B3 retains a no-delete-share Windows directory lease, preventing simple root rename while serving. Nevertheless, current fixture `read/search` still accepts any plain single-link file supplied inside the root when a request occurs, even if that file appeared or its bytes changed after server startup. A pinned root is a *path-identity* boundary, not source-provenance or immutable-content proof. Do not widen tool authority to real workspaces or code execution until this is addressed.

## Required trust model

- **Producer**: a fixed, reviewed developer-lane source snapshot builder operating on the existing CatDesk Git checkout, not a general-purpose shell or arbitrary source-path copier.
- **Source authority**: a concrete 40-character commit SHA on the intended CatDesk Git origin and explicitly designated feature branch. The commit must be independently checked against GitHub's expected branch, and the source snapshot must be derived from that commit's tracked tree objects, not mutable working tree files. Local Git metadata alone is insufficient to assert a reviewed commit or green CI.
- **Source scope**: a fixed reviewed allowlist of Git-tracked regular UTF-8 files, starting narrowly (e.g. `Cargo.toml`, `src/main.rs`), mapped to safe flat fixture leaf names. No caller-selected arbitrary directories; no `.git`, credentials, `.catdesk`, protected production state, system profiles, external tunnel, reparse/symlink/junction, hardlink, hidden name or recursive fetch.
- **Output**: fresh OS-temp direct child named `catdesk-dev-fixture-<generated-id>`; max 32 files, max 512KiB per file and 4MiB aggregate; ephemeral private root. No `config.toml` persisted. The process identity, not ChatGPT input, owns the fixed source manifest.
- **Manifest**: canonical immutable list with `schemaVersion`, `sourceRepository`, `sourceCommit`, `snapshotId`, `files[{fixtureLeaf,sourcePath,sizeBytes,sha256}]`. Sort records, reject duplicates, all digests lowercase 64-hex, compare exact byte length/hash of each copy from a pinned file handle before marking it ready. A runtime-generated manifest only binds contents **since snapshot creation**; it does not certify that its source commit was trusted. Never claim otherwise.
- **Runtime**: require a verified snapshot authority passed from trusted launcher to the isolated fixture (an in-process immutable manifest, not an unauthenticated mutable file). Do not trust arbitrary caller-provided JSON `--manifest` by itself. A fixture read/search must match an allowlisted leaf and recompute SHA-256 from its single open no-follow, link-count-1 file handle before returning text. Missing/new/replaced, changed size/hash, or changed root identity must fail closed with a bounded tool error. Keep all production MCP handlers unchanged.

## Phased implementation tickets

1. **R3B4-A: producer design and fixed manifest schema.** Choose existing source commit/branch verification APIs; implement a deterministic pure validator for a typed manifest and scope (unknown/duplicate fields, filenames, SHA, sizes, aggregate counts). Add unit cases for malformed JSON, tampered hashes, absolute/traversal names, duplicate names, missing commit.
2. **R3B4-B: trusted snapshot builder.** Materialize immutable Git-object bytes from an explicit verified commit into a generated fresh root using no-follow file creation and retained handle writes. Verify back from the same or reopened protected handles, then atomically publish the read-only manifest. Do not read working-tree source by path as provenance. Tests with dirty tracked and untracked files must prove they cannot contaminate a commit snapshot.
3. **R3B4-C: fixture server enforcement.** Bind an immutable expected manifest in the isolated MCP state before accepting requests. Tool endpoint applies existing plain leaf/path denial, pinned/no-follow/unique-handle read, and constant-length/hash comparison. No ability to list or change manifest through MCP. Tests after startup: alter leaf bytes, replace single-link file, append data, introduce a new leaf, inject a hardlink, attempt symlink/junction; read **and** search reject; unchanged expected file remains readable.
4. **R3B4-D: lifecycle acceptance.** Source-current subprocess fixture on OS-temp loopback port0, auth token and process ownership; verify the manifest/commit binding reported only as a nonsecret digest, no production state/tunnel path, and child stop releases root lock. Ensure SIGINT/child kill/timeouts cannot retain a stale trusted lock or expose an orphan listener.

## Acceptance gates

- Windows Rust format, strict all-target/features Clippy, focused fixture/provenance unit tests, real subprocess HTTP tests and full main Rust suite all PASS locally and independently on GitHub Windows CI (Rust, Python Wake/advisor, independent WakeHost Rust).
- Prove unchanged normal CatDesk/headless behavior and external official Secure MCP runtime; no protected signer or installation access, no second tunnel owner or Wake target mutation, no arbitrarily supplied shell path or producer program.
- Security statement remains **limited**: a trusted, bounded *read-only source snapshot*, not a Windows sandbox against hostile same-user processes and not authorization for unrestricted automated coding. A separate restricted agent permission model and bounded owner PID/port restart lifecycle are needed before enabling writes or command tools.
