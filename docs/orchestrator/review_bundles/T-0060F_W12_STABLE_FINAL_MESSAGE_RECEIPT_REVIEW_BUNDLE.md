# T-0060F-W12 Stable Final-Message Receipt Review Bundle

## Scope and diagnosis

The visible W11 automatic wake reached the configured ChatGPT conversation,
but its durable record remained `SUBMITTING` with
`SUBMIT_RECEIPT_SEQUENCE_DRIFT`. W11 still required a surviving predecessor
digest from the pre-submit user-message snapshot. A fully remounted or
virtualized ChatGPT author-message DOM can legitimately expose no such
predecessor, even after the one permitted submit action completed.

This repair changes receipt proof in `scripts/wake_bridge.py` and adds focused
deterministic coverage. It does not retry or mutate the W10/W11 records, open
a browser, modify CatDesk's automatic dispatcher, tunnel, Scheduler,
promotion, provider routing, or Rust receipt validation.

## Receipt proof

Before typing, the bridge obtains the existing bounded user-message digest
snapshot and rejects the action before the submission boundary when the exact
normalized wake-message digest already appears. That snapshot is retained only
for this freshness proof; no prior digest is used to anchor the post-submit
DOM.

After the durable `SUBMITTING` callback and the sole click/Enter action, the
bridge requires all of the following on two distinct bounded polls separated
by a non-zero interval:

- the canonical configured ChatGPT conversation URL is unchanged;
- the composer is empty;
- bounded receipt parsing succeeds; and
- the expected digest occurs exactly once and is the final visible user
  message digest.

The rest of the visible user-message sequence can differ between those polls:
it is not delivery evidence and may have been truncated or fully remounted.
Any failed final-message condition resets the consecutive-observation count.
Malformed query results and target-read failures fail closed immediately;
target drift does likewise. A visible Stop/generating control retains the
existing two-second post-Stop hold, so a receipt is not issued until both the
stability requirement and the hold are satisfied.

Only bounded digest values and fixed diagnostic categories are considered.
No message text, browser storage, profile data, credentials, or arbitrary page
content is persisted or logged.

## Preserved boundaries

- `SUBMITTING` is durably written immediately before one submission action;
  post-boundary ambiguity remains non-retryable `SUBMITTING` operator
  attention.
- `SENT` still requires outer state schema 4 and receipt schema v1 with exact
  record ID, normalized message SHA-256, canonical target SHA-256, and a
  positive browser timestamp. Rust validation is unchanged.
- CatDesk remains the only automatic wake trigger. Codex does not launch a
  browser, and no live browser action was performed for this task.
- Qwen/Ollama remains dormant; the existing Rust
  `transient_codex_429_does_not_activate_local_fallback` regression preserves
  the no-fallback boundary.

## Regression coverage

`tests/test_wake_bridge.py` covers exact-final receipts with no shared
predecessor, two separated stable polls with different remounted histories,
unstable final-digest observations, duplicate/non-final/preexisting expected
digests, target drift, retained/changed composer states, malformed and
oversized bounded receipt results, transient Stop hold behavior, complete
schema-4/schema-v1 receipt binding, and no-post-boundary-resubmit state
handling.

## Verification and handoff

Against the preserved dirty workspace, `cargo fmt -- --check`, `cargo clippy
--all-targets --all-features -- -D warnings`, `cargo test` (476 passed, 18
ignored), and `git diff --check` completed successfully. The existing
dedicated wake-bridge virtual environment was checked as-is but remains unable
to start because its configured Python 3.12 base interpreter is absent; it was
not repaired, replaced, or bypassed. No live browser action was performed.

After independent review and authoritative diff capture, CatDesk may create
one fresh normal automatic canary through its existing dispatcher. Acceptance
requires a new complete schema-4 `SENT` receipt with exact
record/message/target hashes. The W10 and W11 records remain non-retryable;
any fresh post-boundary uncertainty must remain `SUBMITTING` without another
click or Enter action.
