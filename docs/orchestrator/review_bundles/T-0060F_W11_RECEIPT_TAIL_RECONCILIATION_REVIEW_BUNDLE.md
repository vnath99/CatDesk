# T-0060F-W11 Receipt Tail Reconciliation Review Bundle

## Scope and diagnosis

The visible W10 automatic wake reached the exact configured conversation, but
the bridge retained the record in `SUBMITTING` with
`SUBMIT_RECEIPT_SEQUENCE_DRIFT`. The former receipt predicate required the
complete post-submit user-message digest sequence to start with the complete
pre-submit sequence. That rule treated ordinary head-only DOM virtualization
of older conversation rows as a sequence violation after the one permitted
submit action.

This repair changes only receipt comparison in `scripts/wake_bridge.py` and
its deterministic regression coverage. It does not retry, modify, or mark the
existing W10 record as sent; does not invoke a browser; and does not change the
CatDesk dispatcher, automatic trigger, tunnel, Scheduler, provider routing,
or Rust schema-4 validation.

## Fail-closed receipt relation

The post-submit observation is accepted only when all of the following are
true:

- the expected normalized SHA-256 digest was absent in the pre-submit list;
- it occurs exactly once in the post-submit list and is the final entry;
- with a non-empty pre-submit list, the entries before it end in a non-empty,
  unchanged trailing suffix of that prior list; and
- entries before that suffix, if any, are genuinely new values not present in
  the prior list. They can therefore represent only newly materialized head
  rows, not a retained prior row moved around a deletion or reorder.

An empty prior list accepts only the one final expected digest. A non-empty
prior list with no surviving predecessor is rejected as unanchored. The
comparison uses bounded digests only; it persists or logs no message content.

This permits exact append, head truncation, head expansion, and their
combination while retaining the strongest available tail-continuity proof.
It rejects middle deletion/reorder, a changed trailing predecessor, duplicated
or non-final expected values, a preexisting expected digest, malformed bounded
CDP results, and missing anchors. Existing `SUBMITTING` handling remains the
post-boundary terminal state for uncertain receipts, so there is no second
click or Enter attempt.

## Durability and routing boundaries retained

- `before_submit()` still durably records `SUBMITTING` immediately before the
  sole supported click/Enter submission.
- `SENT` still requires the complete schema-4 state plus receipt schema v1,
  exact record ID, normalized message digest, canonical target digest, and a
  finite browser timestamp. Rust continues to validate those fields.
- Exact canonical conversation confinement, one-record ownership, bounded
  result parsing, and non-content-bearing diagnostics remain unchanged.
- Qwen/Ollama remains dormant. The existing Rust
  `transient_codex_429_does_not_activate_local_fallback` regression continues
  to cover that provider boundary.

## Regression coverage

`tests/test_wake_bridge.py` now covers:

- exact append, head truncation, head expansion, and combined stable trailing
  suffix virtualization;
- middle deletion/reorder, changed trailing predecessor, duplicate expected,
  preexisting expected, non-final expected, and zero-anchor ambiguity;
- empty-prior ambiguity, malformed and oversized CDP receipt shapes; and
- schema-4 outer state with a complete schema-v1 receipt accepted by the
  existing exact record/message/target binding test.

## Verification and handoff

Against the preserved dirty workspace, `cargo fmt -- --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` (476
passed, 18 ignored), and `git diff --check` completed successfully. The
existing dedicated wake-bridge virtual environment remains unavailable because
its configured Python 3.12 base interpreter is absent; it was checked as-is
and was not repaired, replaced, or bypassed. No live browser action was
performed by this worker.

CatDesk may independently capture the authoritative diff and, after review,
run one fresh automatic canary through its existing dispatcher. The prior W10
record remains non-retryable. Acceptance requires a fresh complete schema-4
`SENT` receipt with exact record/message/target hashes; any uncertainty after
the durable submission boundary remains `SUBMITTING` with operator attention.
