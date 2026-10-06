# T-0324 R3 diagnostic isolated serving candidate R3

## Classification

`SERVING_CONTROLLER_CANDIDATE_READY` for separate independent final review
only. This isolated binary is candidate evidence; it grants no runtime,
reload, reviewed-build, release, promotion, wake, tunnel, or host authority.

## Accepted authority and terminal boundaries

- Acknowledged authority:
  `review-adc-t0324-r3-reviewed-build-terminal-diagnostic-20260913-6-independent_final_review`.
- Accepted classification: `R3_REVIEWED_BUILD_FAILURE_REPAIRED`.
- The accepted patch preserves the immutable generation
  `c17a0e561cc04c15a9b32f68d35bb824` as terminal
  `BUILD_FAILED_OR_AMBIGUOUS` / `REVIEWED_BUILD_FAILED`; no historical token
  was read, disclosed, or reused.
- R1 is terminal `LEASE_EXPIRED` with zero provider turns. R2 exhausted its
  repair budget after the ordinary workspace output lock. Neither session nor
  any older exhausted/waiting worker was resumed or mutated.

## R2 lock diagnosis

R2's ordinary `CARGO_BUILD_RELEASE` path was unavailable because Windows
denied replacement of `target/release/catdesk.exe`. Read-only process evidence
identified that exact path as the running CatDesk daemon. This R3 task did not
touch that path or process. It used only the fixed isolated target directory,
which is distinct from the locked ordinary workspace release output.

## Fresh isolated candidate

The exact approved profile completed:

```text
cargo build --release --locked --target-dir .catdesk/verification-targets/autonomy-release
```

It initially waited for the isolated build-directory lock, then completed the
release build in 2m 18s. The fixed output was measured read only after the
profile completed:

```text
path:        .catdesk/verification-targets/autonomy-release/release/catdesk.exe
SHA-256:     e4f8561c3eb42f5da93ad9f1f0c623976ee21f4a97c2adde9886e4c720a21183
byte length: 26436096
```

This replaces the prior historical isolated output identity
`054617d87a3994fb1367a7a45b551300f1b1ed400762363d0b7f5bc01c3cbe7f`
(26,408,960 bytes). Read-only binary inspection found the accepted fixed
diagnostic strings `CARGO_DEPENDENCY_NETWORK_UNAVAILABLE` and
`capturedStderrSha256` in the new candidate. This is confirmation that the
bounded diagnostic code is present, not release or runtime authority.

## Verification and attribution

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed.
- `cargo test --workspace --all-targets --all-features` — passed (942 main
  tests; applicable binary and integration targets also passed; existing
  platform-dependent ignored tests remain ignored).
- `CARGO_BUILD_RELEASE_ISOLATED` — passed using the exact fixed command above.
- `git diff --check` — passed; existing CRLF working-copy notices are not
  whitespace errors.
- `git status --short` was inspected. The large pre-existing dirty/untracked
  worktree was preserved. The sole task-attributable source-tree mutation is
  this predeclared bundle; isolated build outputs are verification artifacts.

## Prohibited-action audit and review request

No product source, test, script, or configuration file was edited. No daemon
reload/kill/execution; live PREPARE/CONFIRM/RESULT; promotion or activation;
wake/target/browser action; Secure MCP/tunnel action; protected-state edit;
credential access; signing/elevation; Git staging/commit/publication; or
external-project mutation occurred.

Request independent final review of the acknowledged R3 authority, exact
isolated candidate identity, fixed-profile build result, and no-runtime
boundary. This bundle authorizes no follow-on action.
