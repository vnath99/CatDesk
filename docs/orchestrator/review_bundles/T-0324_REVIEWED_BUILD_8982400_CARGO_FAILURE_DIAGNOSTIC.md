# T-0324 reviewed-build 8982400 Cargo failure diagnostic

## Classification

`CARGO_FAILURE_CAUSE_UNRECOVERABLE_FROM_DURABLE_GENERATION_EVIDENCE`

This is a bounded, evidence-only diagnosis of terminal generation
`8982400e6a444392ac4a51db25c91cc4`. It neither retries nor changes that
generation, and it does not infer a Cargo cause from non-authoritative
workspace comparisons.

## Authority and scope

The generation's immutable attempt binds to acknowledged serving review record
`review-adc-t0324-r3-diagnostic-serving-candidate-isolated-r3-20260913-6-independent_final_review`.
The corresponding serving-candidate review bundle is
`T-0324_R3_DIAGNOSTIC_SERVING_CANDIDATE_ISOLATED_R3.md`. That authority is
used only to identify the reviewed source/build policy; it does not turn a
later workspace build into evidence about this already-terminal generation.

Inspected records were limited to the generation-local `attempt.json`,
`claim.json`, `worker-owner.json`, `result.json`, its materialized source,
isolated Cargo home, and build-artifact metadata. No root-level stale result
was used.

## Durable terminal evidence

The local result records one terminal outcome:

| Field | Durable value |
| --- | --- |
| State | `BUILD_FAILED_OR_AMBIGUOUS` |
| Failure code | `REVIEWED_BUILD_FAILED` |
| Phase | `CARGO_BUILD` |
| Exit code | `101` |
| Classification | `CARGO_EXIT_NONZERO` |
| Captured stderr SHA-256 | `7ef026dc5605af725de77dfb659330e47a6ada463a7eee7a01edec068e8f8881` |
| Captured stderr length | `4096` bytes |
| Stderr truncated | `true` |

The attempt, claim, and worker-owner records are internally consistent with
that one generation. The materialized source contains its Cargo manifest and
the reviewed-build implementation; the only observed local target artifact is
`builds/8982400e6a444392ac4a51db25c91cc4/target/.rustc_info.json`. There is
no generation-local candidate executable, attestation, Cargo stdout/stderr
file, or persisted compiler error text. The isolated Cargo home has only its
marker/cache-directory metadata and no recoverable registry package payload.

## Current Cargo policy and cause analysis

The reviewed-build implementation runs the fixed `cargo build --release
--locked` argv from the materialized source after clearing inherited Cargo/Rust
configuration. It fixes `CARGO_TARGET_DIR` to the generation target,
`CARGO_HOME` to the generation-isolated home, `RUSTC` to the pinned compiler,
and `PATH` to the trusted tool parent. It does not accept caller-selected
paths, tools, environment, hashes, or build arguments.

The terminal record deliberately persists only a bounded diagnostic digest,
length, truncation bit, phase, exit code, and fixed classification. A SHA-256
digest cannot recover its preimage, and the retained 4096-byte prefix is not
present in any generation-local record. Consequently, exit code 101 does
**not** establish whether the failure was dependency availability, resolution,
compilation, linker/output, or another Cargo error. In particular, the sparse
Cargo-home metadata is not sufficient to claim a dependency-network cause.

## Non-authoritative comparison evidence

The approved workspace profiles were run only after the generation was already
terminal and were not a retry:

| Profile | Result | Attribution boundary |
| --- | --- | --- |
| `CARGO_TEST` (`cargo test --workspace --all-targets --all-features`) | passed | Current workspace only; cannot identify the historical generation error. |
| `CARGO_BUILD_RELEASE_ISOLATED` (`cargo build --release --locked --target-dir .catdesk/verification-targets/autonomy-release`) | passed; Cargo emitted a non-fatal home canonicalization warning | Current isolated profile only; it has a different source/build lifecycle and cannot override the terminal result. |
| `GIT_DIFF` (`git diff --check`) | passed | Whitespace check only. |
| `GIT_STATUS` | dirty worktree observed | Pre-existing/inherited state preserved; no causal inference. |

The comparison isolated candidate remains
`.catdesk/verification-targets/autonomy-release/release/catdesk.exe`,
SHA-256 `e4f8561c3eb42f5da93ad9f1f0c623976ee21f4a97c2adde9886e4c720a21183`,
length `26436096`. It is comparison evidence only and grants no reviewed-build,
runtime, release, wake, or tunnel authority.

## Minimum next bounded diagnostic

Do not modify or reuse this generation or its consumed confirmation token. If
a separately authorized fresh generation is needed, the minimum diagnostic
addition is to derive and durably bind a fixed-vocabulary, redacted Cargo
failure class from the complete drained stderr stream (or persist a bounded,
redacted suffix that contains the terminal error), together with the existing
attempt identity, phase, exit code, digest, total length, and truncation
indicator. It must not persist raw environment values, credentials, arbitrary
compiler output, or caller-controlled paths. That future evidence can
distinguish the causal class without weakening the terminal fail-closed result
or reinterpreting this historical generation.

## Attribution and prohibited-action audit

The only task-attributable source-tree mutation is this predeclared bundle.
No product source, test, script, configuration, reviewed-build control record,
or generation-local artifact was edited. No `PREPARE`, `CONFIRM`, `RESULT`,
daemon reload, runtime action, wake/target change, Secure MCP/tunnel action,
Git publication, signing/elevation, or external-project mutation occurred.

## Independent final review request

Request independent final review of this fail-closed evidence conclusion and
of any separately proposed, bounded redacted-diagnostic design before a fresh
reviewed-build generation is considered.
