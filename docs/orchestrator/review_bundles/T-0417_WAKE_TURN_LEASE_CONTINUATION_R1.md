# T-0417 Wake turn-lease continuation R1

## Repair performed

The current dev.52 source contained an incomplete turn-lease merge: runtime
call sites and status readback expected turn helpers that were absent from the
Store. The repair restores a persisted, one-lineage state machine:
`ACTIVE`, `CONTINUE_REQUESTED`, `TIMEOUT_CONTINUE_PENDING`, `COMPLETE`, and
`EXHAUSTED`.

An exact `EXACT_USER_MESSAGE_APPENDED` receipt starts a lease. The Wake adapter
returns transport `SENT` at that receipt; it does not wait for assistant DOM
completion. A bounded regular, no-reparse workspace control file at
`.catdesk/wake-turn-control.json` accepts only schema-1 `COMPLETE` or
`CONTINUE`, with an exact parent event, target generation, and idempotent
control identity. The 20-minute watchdog emits a distinct deterministic
current-generation continuation; it never replays the original event. The
lease clock advances only after that continuation has its own exact receipt.
Continuation count is bounded at eight. The status shape is additive and
includes state, parent, sequence, elapsed/remaining time, soft-checkpoint,
completion, and need-another-turn fields. Existing receiptless
`CHATGPT_NOT_IDLE` handling remains pre-submit only, and historical
`SUBMITTING` records are not eligible for this protocol.

The test-only Store fixture was also switched to the existing scoped test
constructor after the full Wake suite exposed the sandbox-only
`CONFIG_ROOT_UNAVAILABLE` failure. Production `Store::open` and default-root
authority are unchanged.

## Verification evidence

- `cargo test --manifest-path wake/Cargo.toml --lib --all-features`: passed, 26 tests.
- `cargo test --manifest-path wake/Cargo.toml --all-targets --all-features`: passed.
- Focused `turn_` tests: passed, 3 tests, covering idempotent continuation,
  exact-receipt clock reset, watchdog continuation, no parent replay, and
  exhaustion, plus malformed, oversized, and stale control refusal.
- Fixed-runtime adapter suite: passed, 7 tests.
- Fixed-runtime selected bridge suite: passed, 9 tests.
- `cargo build --manifest-path wake/Cargo.toml --release --locked --offline`: passed.
- `cargo clippy --manifest-path wake/Cargo.toml --lib --all-features -- -D warnings`: passed.
- Scoped `rustfmt` was applied to `wake/src/store.rs` and
  `wake/src/runtime.rs`; `git diff --check` passed.

Whole-crate `cargo fmt -- --check` and all-targets strict Clippy remain blocked
by inherited diagnostic-example formatting and Clippy findings outside the
T-0417 source/test surface. No WakeHost install/start, browser action, target
or tunnel mutation, recovery action, Git publication, or external-project
action was performed.
