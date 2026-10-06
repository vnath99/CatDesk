# T-0181 / T-0154-R7D-R3 — Fresh reviewed-build worker and attestation closure

## Scope and prerequisite chain

The cancelled T-0169 work is design history only.  This change starts from the
accepted R7C reviewed-source snapshot authority and retains T-0179/T-0180
restart reconciliation unchanged.  No provider turn, reviewed build, reload,
promotion, recovery, tunnel, browser, Scheduler, external project, or Git
publication was invoked for this implementation.

## Defect addressed

The previous reviewed-build code treated `attempt.json`, `claim.json`, and
`result.json` as loose signals.  In particular, a result file existing was
reported as `BUILD_ATTESTED`; tool paths were recorded without immutable tool
evidence; and promotion parsed a syntax-valid attestation directly.  A copied,
legacy, or substituted record could therefore reach the promotion preflight
without proving the producer chain.

## Trust and state model

`src/reviewed_build.rs` now owns four explicit schemas:

- `ReviewedBuildAttemptV1` schema 2 binds a create-once attempt ID, opaque
  confirmation token, exact R6 record/digest, the complete R7C expected
  snapshot and its three validated identities, fixed tool fingerprints, fixed
  policy, and a digest over the complete attempt.
- `ReviewedBuildClaimV1` binds one owner and spawn generation.  The durable
  `CLAIMED_UNSPAWNED` state distinguishes the crash-before-spawn interval from
  `WORKER_OWNED`; a failed spawn creates terminal failed/ambiguous evidence and
  cannot schedule another owner.
- `ReviewedBuildResultV1` is terminal only when it agrees with the attempt,
  claim owner, and (for success) exact attestation digest.
- `ReviewedBuildAttestationV2` schema 3 is producer-only and domain-bound by
  its self-digest.  It binds attempt, R6 review/session/record digest, all R7C
  snapshot identities, fixed policy/argv/environment digests, Cargo/Rustc
  paths, stable identities, hashes and version hashes, and candidate relative
  identity/length/SHA-256.

`RESULT` no longer trusts file presence.  It parses and validates the terminal
chain, revalidates the snapshot and candidate, and rejects partial, malformed,
tampered, legacy, or drifted records.

## Toolchain, environment, worker, and candidate

Cargo and Rustc are selected solely from fixed absolute host policy slots
(`C:\Program Files\Rust\bin` on Windows), never `PATH`, the workspace, or MCP
arguments.  Every tool is classified as a regular non-link file, has its path
components checked, and is bound to content SHA-256, length, stable metadata
identity, and bounded `--version` evidence at PREPARE.  CONFIRM and the worker
recompute the same evidence; the worker sets `RUSTC` to the attested Rustc.

The build policy is the only allowed argv: `cargo build --release --locked`.
The worker uses `env_clear`, isolated `CARGO_HOME` and `CARGO_TARGET_DIR`, and
only the trusted tool parent on `PATH`; inherited wrappers, flags, target
directories, Cargo configuration selection, and compiler environment cannot
contribute authority.  Snapshot bytes are copied to an isolated source root,
the build destination is isolated, and only the fixed isolated candidate path
`target/reviewed-builds/<attempt>/catdesk.exe` is accepted.  Candidate bytes
are boundedly measured before and after copy.

On Windows, the worker places Cargo in a kill-on-close Job Object before it
waits, so Cargo, rustc, linker, and build-script descendants share one bounded
owner.  The timeout path terminates the job and waits for Cargo.  The public
operation remains closed and is not invoked by this provider turn.

## Promotion binding

`daemon_reload` now calls the shared
`validate_producer_attestation` verifier rather than parsing a promotion mirror
as authority.  PREFLIGHT, CONFIRM, and the promotion helper all recompute the
attempt/claim/result/attestation/snapshot/candidate chain.  The protected R6A
authorization additionally carries and compares attestation digest, attempt
ID, snapshot identities, policy hash, and Cargo/Rustc hashes.  A stale
preflight cannot survive any of these changes.  Legacy schema-2 and
syntax-only records are rejected.

The first-class `catdesk_reviewed_build` and cached
`catdesk_daemon_reload` `REVIEWED_BUILD_PREPARE`, `REVIEWED_BUILD_CONFIRM`, and
`REVIEWED_BUILD_RESULT` bridge retain their exact fixed fields.  They expose no
executable, command, source, target, flag, environment, credential, or shell
input.

## Negative matrix

| Case | Result |
| --- | --- |
| Missing/historical R7C snapshot | `REVIEWED_SOURCE_SNAPSHOT_REQUIRED` |
| Review/snapshot/tool/policy drift | attempt validation fails closed |
| Second/concurrent confirmation | existing owner/result evidence only; no second spawn |
| Crash before spawn | `CLAIMED_PENDING_UNPROVEN`, same attempt only |
| Spawn failure or malformed terminal state | `BUILD_FAILED_OR_AMBIGUOUS` / unavailable |
| Legacy, forged, changed-candidate, or changed-tool attestation | unavailable before promotion authority |
| Candidate path escape, link, size/hash/length drift | unavailable before authorization persistence |
| Caller Rust/Cargo/PATH/wrapper/flags/config input | not accepted by either operation surface |

## Deterministic tests

`reviewed_build` tests now cover fixed policy/argv/environment digest,
worker-argument closure, candidate path escape refusal, immutable
review/snapshot/policy attempt digests, exact one-attempt claim ownership, and
producer attestation rejection for candidate substitution, Rustc substitution,
and legacy schema.  Existing R7C byte-snapshot and T-0179/T-0180 restart tests
remain part of the full suite.

## Changed files

- `src/reviewed_build.rs`
- `src/daemon_reload.rs`
- `docs/orchestrator/review_bundles/T-0181_T0154_R7D_R3_FRESH_REVIEWED_BUILD_WORKER_ATTESTATION_CLOSURE_REVIEW_BUNDLE.md`

## Verification performed

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test` (616 passed, 18 ignored; 2 PowerShell fixtures passed at the
  implementation checkpoint)
- `cargo build --release`
- `git diff --check` and `git status --short` are run for final handoff.

T-0179/T-0180 reconciliation, R7C snapshot authority, R6/R6A/R6B promotion
claim/replay controls, LKG ordering, redaction, `tunnelAction NONE`, and
external Secure MCP non-ownership are preserved.  CatDesk must independently
accept this fresh session before any host-side live reviewed build is allowed.
