# T-0431 — Wake dev.68 immutable package promotion

## Status

DEV68_NATURAL_END_TO_END_ACCEPTED

## Parent authority

T-0430 active-generation refresh/browser-close source logic is verified and
independently accepted by:

- `adc-t0430-wake-refresh-verification-20260927` — COMPLETED_VERIFIED;
- `adc-t0430-r1-independent-refresh-review-20260927` — COMPLETED_VERIFIED;
- `docs/orchestrator/review_bundles/T-0430_R1_ACTIVE_GENERATION_REFRESH_REVIEW.md`.

The parallel Codex packaging review
`adc-t0430-codex-packaging-review-20260927` confirmed the existing immutable
Wake publication/activation path and must be treated as supplemental packaging
evidence, not as live acceptance.

## Narrow source change

The next immutable Wake version is allocated as `1.0.0-dev.68`.

Changed only the three source/lock identity entries:

- `wake/Cargo.toml` package version: dev.67 -> dev.68;
- `wake/Cargo.lock` `catdesk-wake` package version: dev.67 -> dev.68;
- root `Cargo.lock` path dependency version: dev.67 -> dev.68.

No installed Wake package, `current.json`, `previous.json`, target authority,
browser profile, review inbox, recovery/LKG authority, or Secure MCP runtime was
mutated by this version allocation.

## Required verification

Before publication or activation:

1. Re-run the accepted T-0430 fixed Python regression and surrounding response
   completion/Retry/sequence/target/no-duplicate safeguards.
2. Run strict root Clippy and full root Cargo tests.
3. Run Wake package locked/offline release build through the reviewed build path.
4. Verify all four immutable package artifacts:
   `CatDeskWakeHost.exe`, `CatDeskBinagotchy.exe`, `adapter.py`, and
   `wake_bridge.py`.
5. Independently review the exact dev.68 version/package delta.
6. Publish only through the reviewed immutable Wake install path; never overwrite
   dev.67 or copy files directly into the installed versions directory.
7. Activate only through `activate-reviewed-install` and require exact pointer,
   manifest, hash, prior-desired-state, singleton-owner, and restart readback.

## Serving and live-acceptance ordering

T-0429 serving/current parity must be established before resuming the SAME
T-0425 session at stateVersion 7.

After dev.68 is reviewed, published, activated, and read back, live acceptance
must use a fresh legitimate current-generation review event and prove:

USER wake once -> Stop/Pause active -> two-minute bounded same-target periodic
refresh -> exact latest USER revalidated -> Stop/Pause absent/completion proven ->
terminal SENT -> owned browser closes.

Manual test events may be used for diagnostics but do not count as natural
acceptance.
