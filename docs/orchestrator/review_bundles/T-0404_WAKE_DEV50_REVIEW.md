# T-0404 Wake dev.50 reconciliation/lifecycle review

## Classification

`SOURCE_REVIEW_ACCEPTED_FOR_IMMUTABLE_DEPLOYMENT_WITH_RUNTIME_PROOF_REQUIRED`

This review covers source and compile evidence only. It does not treat the ambiguous
T-0403 Wake as delivered and it does not clear, downgrade, replay, retire, or retarget
that event.

## Incident being repaired

Installed dev.49 attempted review
`review-adc-t0403r2-launch-recovery-tunnel-consolidation-20260922-6-independent_final_review`.
The first browser crossed the durable `BEFORE_SUBMIT -> SUBMITTING` boundary but no
`EXACT_USER_MESSAGE_APPENDED` receipt was proven. One later observe-only reconciliation
browser also failed to prove the expected digest. Live dev.49 therefore remains fail-closed
at `SUBMITTING / SUBMISSION_RECONCILIATION_REQUIRED`; the last proven receipt is T-0402R1.

## dev.50 repair

### One browser request per independent adapter

`wake/adapter.py` no longer keeps a SeleniumBase context alive in a `while True`
request loop. One adapter process now accepts exactly one attempt or one observe-only
reconciliation request and then returns from `with SB(...)`.

Consequences:
- successful normal `SENT` exits the owned SeleniumBase context immediately;
- successful observe-only reconciliation exits the owned context immediately;
- failed attempts already return through the exception path;
- the Rust parent retains its Job/child cleanup as a bounded backstop;
- no broad Chrome/process kill is introduced.

### Rust-side success cleanup

`wake/src/runtime.rs` now retires the adapter after normal `SENT` and after successful
reconciliation. Status distinguishes graceful versus forced cleanup. This is redundant
with the adapter context exit by design and closes the process/job ownership boundary.

### Ambiguous SUBMITTING quarantine

An exact delivery already in `Phase::Submitting` remains immutable with respect to replay:
- `Store::claim` still refuses it;
- `retry_pre_submit` still refuses it;
- target changes remain refused while a SUBMITTING delivery exists;
- reconciliation can still only advance `SUBMITTING -> SENT` after an exact bound receipt.

New `Store::note_submitting_attention` persists only a valid fixed-vocabulary failure
reason while preserving `Phase::Submitting`, a null receipt, the same event/target binding,
and the same queue entry. It grants no retry authority.

After one observe-only reconciliation attempt in a host lifetime, that exact event is
quarantined/skipped rather than causing the event loop to break. Later distinct queue events
may therefore be handled without resubmitting, downgrading, deleting, or retargeting the
ambiguous event.

The persisted specific reason is used for later status instead of immediately replacing it
with the generic `SUBMISSION_RECONCILIATION_REQUIRED` label.

### Installer version authority

`wake/install.ps1` no longer contains a manually maintained package version constant.
It derives the version from the `[package]` version in `wake/Cargo.toml` and refuses a
value outside the fixed `1.0.0-dev.<integer>` form. Candidate activation still checks
Rust publication/readback against that derived version.

### Version

Wake source/locks are advanced to `1.0.0-dev.50`.

## Regression coverage

A store regression now proves that a specific reconciliation failure reason can be persisted
on SUBMITTING while:
- the phase remains SUBMITTING;
- receipt remains absent;
- `claim` remains refused with `SUBMISSION_RECONCILIATION_REQUIRED`;
- `retry_pre_submit` remains refused;
- reread preserves the exact reason.

Existing reconciliation tests still bind successful reconciliation to an exact receipt and
prove mismatched receipts leave SUBMITTING unchanged.

The Python direct/manual bridge close helper introduced in dev.49 remains harmless for that
separate path; production independent Wake now closes through the one-request adapter context
plus Rust retirement.

## Verification evidence

Passed:
- `cargo build --tests --locked --offline --manifest-path wake/Cargo.toml`
  - compiled `catdesk-wake v1.0.0-dev.50` successfully.
- `cargo build --release --locked --offline --manifest-path wake/Cargo.toml`
  - compiled optimized dev.50 host successfully.

Not claimed:
- `cargo test` execution: the generic MCP command wrapper currently rejects the invocation
  before a test result is returned.
- standalone Python test execution / py_compile: the same generic command wrapper rejects
  those invocations.
- full workspace fmt/clippy/diff execution through generic shell: exact advertised commands
  are currently refused by the wrapper. This is a control-plane command-gate inconsistency,
  not a claimed PASS.

Rust test targets compile, so the new Rust regression compiles. Runtime activation therefore
requires a fresh post-install status check and a later fresh Wake event to prove delivery,
browser closure, and continued processing with the quarantined T-0403 event present.

## Deployment constraints

- Preserve the current T-0403 SUBMITTING journal unchanged.
- Do not manually retire/downgrade/replay T-0403.
- Immutable publication/activation only.
- A dev.50 restart may perform one observe-only reconciliation of T-0403 because the
  in-memory reconciliation-attempt guard starts fresh; that operation must remain non-sending.
- After that reconciliation is quarantined, later distinct events must remain processable.
- Final close-after-success acceptance requires a fresh distinct Wake with a durable
  `EXACT_USER_MESSAGE_APPENDED` receipt and observation that the session-owned browser exits.
