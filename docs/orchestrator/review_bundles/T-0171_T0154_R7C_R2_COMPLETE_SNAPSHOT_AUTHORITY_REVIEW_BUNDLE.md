# T-0171 / T-0154-R7C-R2 complete snapshot authority

## Rejected R7C-R1 shape

T-0170 corrected the producer/consumer pathname mismatch, but both sides still
interpreted a small JSON manifest independently.  That shape was not bound to
the logical task, attributed output evidence, or byte-object contents, so a
nonempty manifest could not establish reviewed-source authority.

## Shared authority implementation

`src/reviewed_source_snapshot.rs` is now the sole producer/consumer authority.
It exposes only create-or-validate and validate-existing operations, an exact
expected-authority value, and a validated result.  The canonical layout is:

```text
.catdesk/reviewed-source-snapshots/<session-id>/
  manifest.json
  bytes/<normalized-workspace-relative-input>
```

The v4 manifest binds `sessionId`, `projectId`, approved contract hash, exact
logical task id, sorted structured completion artifact ids, current attributed
output hashes, and immutable baseline observations.  The shared module derives
the authority digest itself; callers cannot supply one.  It computes:

- `authorityDigest = SHA-256(CATDESK_REVIEWED_SOURCE_AUTHORITY_V1 | canonical authority fields)`
- `manifestDigest = SHA-256(CATDESK_REVIEWED_SOURCE_MANIFEST_V1 | schema | authorityDigest | ordered entries)`
- `snapshotId = SHA-256(CATDESK_REVIEWED_SOURCE_SNAPSHOT_ID_V1 | authorityDigest | manifestDigest)`

Each entry records normalized path, length, SHA-256, and its exact `bytes/`
content-object identity. Validation recomputes all three digests, rehashes every
object, and rejects missing, extra, tampered, linked, special, or malformed
objects. Legacy `<session>.json` hash-only state is never read and remains
`REVIEWED_SOURCE_SNAPSHOT_REQUIRED` at the promotion boundary.

## Source coverage and safety

The capture inventory requires root `Cargo.toml` and `Cargo.lock`, accepts only
a safe optional `build.rs`, recursively includes all regular `src/**` files,
and audits literal `include_str!`/`include_bytes!` references. Literal
project-local assets are included; dynamic or ambiguous include forms fail
closed. `.git`, `.catdesk`, `target`, absolute paths, parent traversal,
non-UTF-8 identities, symlinks/reparse points, special files, duplicate or
case-fold-colliding paths, and metadata ambiguity are rejected.

Bounds are 512 normalized path bytes, 4,096 entries, 16 MiB per object, and
64 MiB aggregate content. Entries and directory enumeration are lexical and
overflow checked.

## Atomicity and completion ordering

Capture stages binary bytes and a fully validated manifest under a private
directory, then atomically renames only the validated final directory. Partial
staging is not authority. An existing final directory is strictly validated;
an exact replay succeeds without reading changed live source bytes, while any
mismatch fails closed without regeneration.

The controller now performs: independent verification PASSED → output
attribution → expected snapshot derivation → snapshot create/validate → durable
task completion → completion/final-review artifacts → review inbox. Snapshot
failure therefore cannot emit completion or review authority.

`daemon_reload` now calls the same validator using exact review/session/task
output evidence reconstructed by the supervisor rather than parsing its own
manifest shape.

## Deterministic coverage and verification

New production-module tests cover nested byte round-trip and replay after live
source drift, manifest tamper/authority drift, dynamic include rejection,
literal include-asset capture, extra/legacy state rejection, and immutable
completion-output binding. The complete Rust suite ran with 613 tests: 595
passed, 18 ignored, 0 failed;
the two project PowerShell recovery fixture tests also passed. `cargo fmt
--check`, `cargo clippy --all-targets --all-features -- -D warnings`, and the
fixed `cargo build --release` profile passed during implementation.

Changed task artifacts:

- `src/reviewed_source_snapshot.rs`
- `src/delegated/autonomous_controller.rs`
- `src/daemon_reload.rs`
- `src/delegated/autonomy_supervisor.rs` (exact review evidence adapter)
- `src/main.rs` (module registration)
- this bundle

No build worker, toolchain resolver, candidate creation, attestation producer,
promotion, reload, recovery, tunnel, browser, Scheduler, external-project
mutation, or Git publication was performed. T-0169/R7D remains deferred until
host independently accepts this R7C-R2 snapshot authority layer.
