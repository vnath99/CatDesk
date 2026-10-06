# T-0324 WEB current serving-candidate R3

## Classification

`SERVING_CONTROLLER_CANDIDATE_READY` for separate independent final review
only. This ordinary workspace binary is candidate evidence, not deployment,
promotion, reload, runtime, or reviewed-build authority.

## Accepted authority

- Terminal-failure repair review:
  `review-adc-t0324-reviewed-build-terminal-failure-diagnostic-r2-20260913-6-independent_final_review`.
- Terminal-repair snapshot:
  `5e67e86cdae2ee8d8c6c455140c0f2cf693de3b624b9321c6656be623a3a768a`.
- Terminal-repair manifest:
  `c82c1cc0b83848c409039214b7ed5e8041e80cf556c9e065068491cec8db11b7`.
- WEB compatibility identity:
  `sha256:725505e72b85614e6af8ab829f4ef6414c40614635d9cbd88f64d616d9629880`.

Current source retains the bounded uppercase `WEB:<legacy-id>` target grammar
and the reviewed-build repeated-parent `descend_or_create` repair. No product
source, test, script, or configuration file was changed.

## Ordinary workspace candidate

The sole build command was the existing `CARGO_BUILD_RELEASE` profile:

```text
cargo build --release
```

It succeeded. The freshly measured candidate is:

```text
path:        target/release/catdesk.exe
SHA-256:     fe50ee4ee8a2838f0dd04ed5d7929eebe1bc63e923c44b0b5c003fad55332a37
byte length: 26408960
```

The locked isolated running binary was not read, measured, modified, or used as
candidate authority. Its historical SHA-256
`054617d87a3994fb1367a7a45b551300f1b1ed400762363d0b7f5bc01c3cbe7f`
differs from this new ordinary-build evidence and remains historical only.

## Verification

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `cargo test --workspace --all-targets --all-features` — passed: main crate
  `919 passed; 0 failed; 21 ignored`, plus all binary and integration targets.
- `cargo build --release` — passed.

## Attribution and boundaries

Only this predeclared review bundle is task-attributable in the source tree.
The existing dirty/untracked worktree was preserved; `target/release` is build
evidence only. No daemon reload/execution; live PREPARE/CONFIRM/RESULT;
promotion; activation; wake, target, browser, tunnel, or Secure MCP action;
protected-state edit; signing/elevation; Git publication; or external-project
mutation occurred.

Independent final review must inspect this bundle and exact candidate identity
before any separately authorized serving decision. This task authorizes none.
