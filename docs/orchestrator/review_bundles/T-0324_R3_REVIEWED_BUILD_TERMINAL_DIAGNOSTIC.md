# T-0324 R3 reviewed-build terminal diagnostic

## Classification

`R3_REVIEWED_BUILD_FAILURE_REPAIRED`, pending independent final review.

This is the sole predeclared completion artifact in contract
`adc-t0324-r3-reviewed-build-terminal-diagnostic-20260913`. It repairs the
bounded failure-observability defect; it does not convert the terminal
generation into an attested build or authorize any retry.

## Authoritative terminal evidence and localization

Generation `c17a0e561cc04c15a9b32f68d35bb824` was read only. Its protected
attempt, claim, and owner proof agree, and its immutable result is exactly
`BUILD_FAILED_OR_AMBIGUOUS` with failure code `REVIEWED_BUILD_FAILED`. Its
reviewed-source snapshot binding is
`4f87ff5cd155b4949861b25f51c14481f49e1307b0d27694c52a7ade867557d6` under
`adc-t0324-web-current-serving-candidate-review-r3-20260913`.

The materialized source tree exists beneath that generation's protected build
directory, so this failure is after snapshot materialization. The accepted
repeated-parent repair is present in current `materialize_snapshot`: each
target parent uses the shared pinned no-follow `descend_or_create` transition,
and `materialize_snapshot_reopens_repeated_scripts_parent` exercises two
independently measured `scripts/` children through that production helper.
Thus the observed result is neither the prior snapshot-child collision nor an
output-path/attestation failure: those occur only after Cargo returns success.

The fixed worker invokes the pinned Cargo with `build --release --locked`,
clears inherited Cargo/Rust configuration, uses an isolated target directory,
and sets `CARGO_HOME` to the protected generation control `cargo-home`.
That home contained only its cache marker, with no registry index or package
cache. A disposable source-local diagnostic probe against the immutable
materialized source observed bounded Cargo retries for `index.crates.io`
connection failure before it was stopped; no reviewed-build control file or
protected runtime state was touched. This corroborates unavailable dependency
resolution in the local build environment. The precise fail-closed diagnosis
of the historical generation is a nonzero Cargo exit whose original stderr was
discarded; it cannot retrospectively prove a more specific compiler message.
The empty exact isolated Cargo home and the local registry-connectivity signal
are the strongest available explanation, not replacement authority for the
immutable terminal record.

## Minimal repair

`src/reviewed_build.rs` now pipes and continuously drains Cargo stderr without
retaining raw output. On a nonzero Cargo exit only, it writes an optional
`failureDiagnostic` into the create-once terminal result:

- fixed `schemaVersion: 1` and phase `CARGO_BUILD`;
- optional process exit code;
- one fixed classification: `CARGO_DEPENDENCY_NETWORK_UNAVAILABLE` when the
  bounded capture detects the registry-connectivity shape, otherwise
  `CARGO_EXIT_NONZERO`;
- SHA-256, retained-byte length, and truncation flag for at most 4096 stderr
  bytes.

It persists no stderr text, source path, URL, environment value, credential,
tool choice, or caller input. The reader drains the complete pipe so bounded
retention cannot block Cargo. Successful Cargo exits discard the capture and
continue through the existing post-build evidence checks unchanged. Existing
results remain readable because the new field is optional and defaults absent.

The repair preserves the exact source/review binding, pinned tool checks,
RootDirectory/no-follow snapshot materialization, one-worker owner proof,
create-once result semantics, consumed-token rejection, terminal history, and
fail-closed output/attestation gate. It neither changes the failed generation
nor supplies a new token, authority, cache, network path, output path, or
attestation.

## Focused regressions

- `cargo_failure_diagnostic_redacts_network_failure_and_bounds_capture`
  drains an over-bound input, classifies the registry connection shape, proves
  truncation, and proves serialized evidence contains neither a synthetic
  credential nor the raw registry host.
- `cargo_failure_diagnostic_keeps_non_network_failure_generic` preserves the
  generic nonzero-Cargo classification for unrelated compiler/linker failure.
- Existing `materialize_snapshot_reopens_repeated_scripts_parent` remains the
  direct production-path regression for the accepted snapshot repair.

## Verification

- `cargo test cargo_failure_diagnostic -- --nocapture` — passed (2 focused
  tests; 940 filtered main-crate tests).
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed.
- `cargo test --workspace --all-targets --all-features` — passed (942 main
  tests; applicable binary and integration targets also passed; the existing
  platform-dependent ignored tests remain ignored).
- `git diff --check` — passed. Git emitted existing CRLF working-copy notices
  only.

## Attribution, audit, and next boundary

This workspace already has a large dirty/untracked worktree, preserved without
reset, staging, commit, or modification of unrelated files. The attributable
implementation is the bounded diagnostic addition and its two focused tests in
`src/reviewed_build.rs`; this review bundle is the sole predeclared completion
artifact. No live reviewed-build attempt/control state, generation, claim,
owner proof, result, or historical confirmation token was changed or reused.

No daemon reload/kill; live PREPARE/CONFIRM/RESULT; promotion; activation;
wake/target/browser action; Secure MCP/tunnel action; protected release-state
mutation; signing/elevation; Git publication; credential access; or
external-project mutation occurred.

After independent final review, the next bounded action is external dependency
availability remediation through an independently authorized host/build-cache
operation, followed by a wholly fresh PREPARE and only its newly returned
opaque confirmation token. It must advance only if RESULT is
`BUILD_ATTESTED`; neither this diagnosis nor the terminal generation grants
that authority.

## Independent final review request

Request independent final review of the exact diagnostic schema, redaction and
bounded-drain behavior, source-local failure classification, focused
regressions, and preserved fail-closed reviewed-build lifecycle.
