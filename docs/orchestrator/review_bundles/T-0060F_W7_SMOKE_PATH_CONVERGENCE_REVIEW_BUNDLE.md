# T-0060F-W7 Smoke Path Convergence Review Bundle

## Diagnosis

The standalone SeleniumBase smoke paths had already produced
`SMOKE_TEST_SENT`, while the production bridge continued to report
`SUBMIT_RECEIPT_UNPROVEN`. The W6 bridge had diverged by inventing a temporary
DOM-marker click route. That route was not used by either smoke script and did
not include the current `button#composer-submit-button` compatibility selector.

The reviewed CDP smoke path instead:

- uses the fixed current send selector set directly with `cdp.click()`;
- includes `button#composer-submit-button`;
- treats multiple selector matches for the same control as compatible; and
- sends native Enter only when no safe send button is available.

## Production convergence

`scripts/wake_bridge.py` now follows that established CDP submission behavior.
It selects from the smoke-compatible fixed selector set, de-duplicates matches
by exact DOM identity, rejects multiple distinct visible controls and disabled
controls, then calls `cdp.click()` directly on the selected fixed selector.
The temporary marker mechanism is removed.

If there is no visible send control, the bridge uses `cdp.press_keys` with the
native Enter key only after it has verified the exact normalized wake message
in the composer immediately before the durable `SUBMITTING` transition. An
ambiguous or disabled control is never converted into an Enter fallback.

The production path remains stricter than either smoke test:

- the exact review record is claimed before browser work;
- `SUBMITTING` is saved immediately before click/Enter;
- all click/Enter errors remain post-boundary operator attention, without a
  blind retry;
- `SENT` still requires an empty composer, exactly one appended normalized
  user-message digest after the unchanged prior sequence, positive browser
  timestamp, exact message and target hashes, and schema-4 receipt version.

Exact conversation validation and dedicated-profile confinement are unchanged.

## Deterministic coverage

The Python test source covers direct compatible click success,
`composer-submit-button`, same-element selector de-duplication contract,
distinct/disabled control rejection, native Enter success only for a missing
control, click/Enter failures, exact receipt success, receipt mismatch/sequence
failure, typed-unsent behavior, and post-boundary no-resubmit behavior. It also
checks both standalone smoke scripts retain the same compatibility selector and
CDP click/Enter path.

## Verification

- `cargo fmt -- --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test`: 476 passed, 18 ignored.
- `git diff --check`: passed after this bundle.
- Project-local Python wake tests were attempted but the venv executable still
  references a missing Python 3.12 base interpreter. The environment was not
  repaired, replaced, or bypassed.

## Residual live acceptance

After independent review and candidate loading, allow only the ordinary
CatDesk automatic dispatcher to generate one fresh read-only wake canary for
the already configured exact conversation. Success requires the complete
schema-4 receipt; any fixed attention state requires operator review and no
manual/blind retry.

No live browser action, wake, daemon reload/promotion, tunnel or Scheduler
action, browser-storage/credential access, or Git publication was performed.
