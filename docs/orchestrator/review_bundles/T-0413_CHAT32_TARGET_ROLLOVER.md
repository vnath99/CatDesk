# T-0413 Chat32 Wake Target Rollover Review

## Classification

`CHAT32_TARGET_ROLLOVER_SCOPE_VERIFIED` - the target-rollover semantics and
their scoped regression coverage are verified. Workspace-wide formatting remains
blocked by inherited, unrelated formatting drift; it was not altered by this
bounded task.

## Evidence reviewed

- T-0403R2 is a completed historical authority record (`COMPLETED_VERIFIED`)
  and is treated as the source of the prior `SUBMITTING` ambiguity contract,
  not as mutable current delivery state.
- `wake/src/store.rs` keeps ordinary `Store::set_target` on the non-rollover
  branch. A same-project `SUBMITTING` delivery returns
  `CONFIG_SUBMISSION_RECONCILIATION_REQUIRED` before target mutation.
- The separate rollover branch requires the exact current generation, retains
  the old delivery target in current config/history, and does not edit the
  delivery record or queue event. Missing retained history fails closed with
  `CONFIG_SUBMISSION_TARGET_UNAVAILABLE`.
- `src/mcp.rs` exposes the special behavior only through the private helper
  used by `operator_update_designated_chat_target`; the ordinary
  `catdesk_wake_target_set` path continues to call the ordinary helper.

## Transaction and immutability findings

`operator_update_designated_chat_target` canonicalizes the candidate, requires
an exact displayed SHA-256 CAS, serializes with `WAKE_TARGET_SET_LOCK`, and
requires project/Wake readback coherence before its first write. It then holds
the registry lock across the Wake stage and project-target CAS. Preexisting
registry divergence, duplicate workspace bindings, malformed digests, and
stale readback fail closed.

If the registry CAS returns an error after Wake staging, the code attempts an
exact Wake compensation under the same lock and rechecks paired readback. A
failed compensation or ambiguous readback returns `SynchronizationFailure`;
an interrupted transaction is therefore a divergence that later readback
rejects, not a ready target authority.

The store's rollover tests prove that the prior `SUBMITTING` delivery retains
its original target, phase, reason, and absent receipt; its original queue event
remains. It cannot be reclaimed or replayed because its generation is stale and
the claim path rejects non-`Claimed` delivery state. A distinct generation-two
event is separately claimed, submitted, and committed `SENT` with an exact
receipt, without changing the quarantined old record.

## Attributable repair

The only change made in this review is in `wake/tests/protocol_store.rs`.
The deterministic fixture now uses `Store::open_scoped_for_test` beneath an
existing canonical fixture parent. This is the established test-support seam
needed in the provider sandbox, whose ancestor inspection prevents production
`Store::open` from opening ordinary temporary roots. Production
`Store::open` and the rollover implementation were not changed.

## Verification

| Check | Result |
| --- | --- |
| `cargo test --manifest-path wake/Cargo.toml --features test-support --test protocol_store` | PASS: 24 passed, 0 failed |
| `cargo test --workspace --all-targets --all-features designated_chat_target -- --nocapture` | PASS: 3 passed, 0 failed |
| `cargo test --workspace --all-targets --all-features` | PASS: primary suite 954 passed, 0 failed, 21 ignored; all additional targets/integration suites passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS |
| `rustfmt --edition 2024 --check wake/tests/protocol_store.rs` | PASS |
| `cargo fmt --all -- --check` | BLOCKED by pre-existing formatting drift in unrelated files and inherited rollover-delta source; the scoped fixture file itself passes `rustfmt --check` |
| `git diff --check` | PASS (line-ending warnings only) |

`git status --short` reported 156 inherited worktree entries. The scoped status
was the existing `src/mcp.rs` modification plus untracked
`wake/src/store.rs` and `wake/tests/protocol_store.rs`; this review adds this
bundle and the bounded protocol-store fixture correction only. No live Wake,
target, queue, receipt, tunnel, reviewed-build, release/LKG, daemon, Git, or
external-project state was mutated.

## Residual risk

The cross-store operation cannot make two independently durable filesystems
physically atomic. Its designed failure mode is deliberately fail-closed:
post-stage registry failure is compensated when control returns, and an
interruption before compensation leaves detectable project/Wake divergence that
blocks future authority readback. Live delivery remains outside this review.
