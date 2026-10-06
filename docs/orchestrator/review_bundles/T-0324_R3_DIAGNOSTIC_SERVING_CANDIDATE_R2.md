# T-0324 R3 diagnostic serving candidate R2

## Classification

`SERVING_CONTROLLER_CANDIDATE_DEFECT`. A fresh ordinary-workspace candidate
cannot be established while the fixed `target/release/catdesk.exe` output is
the running daemon and Windows denies Cargo replacement. This classification
grants no runtime, serving, reviewed-build, release, promotion, wake, tunnel,
or host authority.

## Accepted authority and terminal history

- Acknowledged diagnostic authority:
  `review-adc-t0324-r3-reviewed-build-terminal-diagnostic-20260913-6-independent_final_review`.
- Its persisted review record is `COMPLETED_VERIFIED` and its accepted
  classification is `R3_REVIEWED_BUILD_FAILURE_REPAIRED`.
- The accepted source change is the bounded Cargo terminal-diagnostic patch in
  `src/reviewed_build.rs`; it preserves the pre-existing repeated-parent
  snapshot-materialization repair.
- Generation `c17a0e561cc04c15a9b32f68d35bb824` remains immutable terminal
  `BUILD_FAILED_OR_AMBIGUOUS` / `REVIEWED_BUILD_FAILED`. Its historical
  consumed token was not read, disclosed, or reused.
- The previous R1 serving-candidate session is terminal `LEASE_EXPIRED` with
  zero provider turns. It was not resumed or mutated.

## Ordinary workspace output and bounded failure

The fixed existing `CARGO_BUILD_RELEASE` profile was run:

```text
cargo build --release
```

An earlier local invocation completed and the ordinary workspace output was
measured read only as:

```text
path:        target/release/catdesk.exe
SHA-256:     fe50ee4ee8a2838f0dd04ed5d7929eebe1bc63e923c44b0b5c003fad55332a37
byte length: 26408960
```

Independent verification subsequently reran the required profile and Cargo
failed before a fresh candidate could be produced:

```text
failed to remove target/release/catdesk.exe: Access is denied (os error 5)
```

Read-only process evidence identifies the exact lock owner as the running
`catdesk` process (PID `46856`, observed start time 2026-09-13 10:45:53) at
the same path. Its SHA-256 and length match the values above. This proves the
lock condition; it does not make the previously measured bytes fresh evidence
for this retry. The process was not stopped, reloaded, signaled, or otherwise
mutated.

## Dependency-availability observation

The accepted R3 diagnostic established that a reviewed-build generation's
isolated `CARGO_HOME` lacked cached dependencies and that raw stderr had been
discarded. The contract records that the host's locked dependency availability
was subsequently remediated by a successful `cargo fetch --locked`. This task
did not perform that fetch, does not infer reviewed-build success from it, and
does not grant any new build, source, runtime, or review authority from cache
availability. The ordinary release build above is the only new candidate
evidence in this session.

## Verification

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed.
- `cargo test --workspace --all-targets --all-features` — passed (942 main
  tests; applicable binary and integration targets also passed; pre-existing
  platform-dependent ignored tests remain ignored).
- Initial local `cargo build --release` — completed before the subsequent
  daemon lock was observed.
- Independent required `cargo build --release` retry — failed closed with the
  exact Windows output lock above; no replacement candidate was accepted.
- `git diff --check` — passed. Existing CRLF working-copy notices are not
  whitespace errors.
- `git status --short` was inspected before and after verification. The large
  existing dirty/untracked workspace was preserved; no product-source, test,
  script, or configuration mutation is attributable to this session.

## Attribution and prohibited-action audit

The sole task-attributable source-tree mutation is this predeclared review
bundle. `target/release` is ordinary build output, not accepted fresh candidate
evidence and not an attributable source-tree change. No daemon reload/kill;
live
reviewed-build PREPARE/CONFIRM/RESULT; release promotion or activation;
wake/target/browser action; Secure MCP/tunnel action; protected-state edit;
credential access; signing/elevation; Git staging/commit/publication; or
external-project mutation occurred.

## Independent final review request

Request independent final review of the acknowledged R3 diagnostic authority,
the exact output-lock evidence, and the preserved no-runtime boundary. A
separately authorized guarded daemon handoff or stop is required before a new
ordinary-workspace release candidate can be built and measured. No follow-on
action is authorized by this bundle.
