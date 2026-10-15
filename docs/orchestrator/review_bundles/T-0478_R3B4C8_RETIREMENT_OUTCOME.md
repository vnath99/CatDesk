# T0478 R3B4-C8 — Retirement write-ahead and terminal outcome

Date: 2026-10-14. Source-only lifecycle increment; no installed image, WakeHost, browser or tunnel changed.

## Change

The consuming, exact-handle snapshot retirement now keeps the independent owner-only journal alive through the whole transaction. After manifest, retained-handle and protected-root preflight, it writes a create-only, file-synced `retirement-started.json` bound to the canonical serving generation. No file disposition begins if this marker cannot be read back. It then performs the existing exact opened-file and root disposition. A create-only `retirement-outcome.json` records `retired` on verified root absence or `attention` on failure. Both records can be read against the manifest and canonical journal after the root no longer exists; they do not create cleanup authority.

If preflight fails before the start marker, the owner attempts fixed-code `retirement_failed` runtime attention while the root is still pinned. An outcome-write failure is returned explicitly, including the case where the root was already retired. The journal remains for forensic inspection in all cases.

## Verification

- Focused Windows success test passed: exact manifest file and root are disposed; the reopened journal retains matching start and `retired` records after root absence.
- Focused Windows replacement test passed: a same-name replacement at the root DELETE reopen boundary is preserved, the original empty root remains at its moved path, and the journal records `attention` tied to the start generation.
- Strict Clippy passed. Full local `cargo test --all-features --quiet` exited 0: main binary 1,086 passed/26 ignored and all subsequent integration groups passed. Rustfmt and diff whitespace checks passed.
- Positive witnessed launch still requires exact-commit three-job GitHub Windows CI. Workflow dispatch currently reports Actions disabled.

## Remaining acceptance

Retire the separate journal using exact opened handles after a clean terminal outcome; establish cross-process owner exclusion, bounded PID/port supervision, and power-loss recovery semantics. Then exercise an authenticated, witnessed launch and stop. P0 WakeHost installation/fresh canary, P1 Codex review cycle, later multi-project scheduling and T0484 remain open.
