# T-0483 R34 — Stopped Sol direct-parent ACL diagnostic (source only)

## Change

The inert stopped-worker candidate can now audit a caller-supplied pinned staging root and its direct parent. It brackets an exact host/System-only ACL check with retained exited-worker process/token/fixed-account checks and another empty-Job readback. The two opened directory objects must have an exact direct-parent relation. A fixture with an explicit Everyone access entry on the supplied parent fails even when the child root is private.

The diagnostic returns only `Result<(), String>`. The supplied parent is not necessarily the entire ancestor chain, and a present-day DACL cannot rule out a writable handle copied earlier or an escaped process outside the Job. It grants no stage, burn, repository-use, review or Wake authority.

## Verification

- Focused Sol suite passed 87 tests with two ignored helpers; the revised staging suite passed 14. Strict workspace Clippy, formatting and scoped diff checks passed. Independent source review found no blocker and confirmed the broad-parent fixture exercises ACL rejection.
- On the settled combined source, the isolated root shell/cwd regression passed. The remaining all-features root suite passed 1,231 main tests with 28 ignored plus binaries/integrations and nested Wake. Strict root/Wake Clippy and formatting, root all-bin build and staged diff checks passed locally.

Source commit `0ffd8d53015168a2fc9d2e8e4f2eaa5da1880b37` was pushed. Immediate exact-SHA GitHub readback found zero check-runs, and the newest visible workflow run still targeted older source `4e9a2e2`. No remote CI acceptance is claimed.

## Remaining gate

The `CatDeskSolWorker` account and trusted primary-token provider are absent, so no positive fixed-account run occurred. Earlier ancestors, outside-Job descendants, copied writable handles and later ACL changes remain unproven. Durable burn-to-stage, a genuine Sol/medium deliverable, natural `review_ready` and an exact Wake receipt remain open.
