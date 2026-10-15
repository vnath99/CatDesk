# T0483 R8 — Autonomous review inbox replay binding

Date: 2026-10-14. Scope: source-only autonomous review inbox; no live task or Wake event.

## Change

The autonomous inbox previously returned any existing record with the same generated ID without comparing the requested evidence. A retry could therefore appear successful after a project, state, or artifact reference changed. The store now builds and validates the requested record, then compares all stable fields against an existing same-ID record before returning it. The first durable record owns its creation time and acknowledged/unread state; a retry with a later clock value preserves both. A conflicting project, state, reference, session or action fails closed and leaves the inbox unchanged.

The controller checkpoint test models a crash after inbox write and before `COMPLETED_VERIFIED`: retry finalizes from the existing checkpoint without another provider turn or duplicate review, preserving the original record.

## Verification

- Focused inbox tests prove exact replay before and after acknowledgment, later-clock preservation, and rejection of same-ID project/state/reference changes.
- Controller checkpoint retry test proves the partial-finalization case.
- Focused delegated state and controller checkpoint tests pass. `cargo fmt --check`, `git diff --check`, strict offline workspace Clippy, and full `cargo test --locked --offline --all-features --quiet` pass. Main Rust suite: 1,091 passed, 26 ignored; all following binary/integration suites exited 0. Existing AppContainer child probes print access-denied diagnostics while their parent tests pass.
- `pytest tests/test_wake_bridge.py tests/test_stable_wake_browser_adapter.py -q` exited 0 (one skipped); no live Wake or browser submission was attempted.

## Boundary

The review record still references a generic completion artifact path. It does not itself bind an immutable source SHA, file list, verification digest, effective Sol/medium turn telemetry, or an exact installed Wake delivery. Those are required before an authentic T0483 review cycle can be accepted. No live session, WakeHost, installed binary, browser or tunnel state changed.
