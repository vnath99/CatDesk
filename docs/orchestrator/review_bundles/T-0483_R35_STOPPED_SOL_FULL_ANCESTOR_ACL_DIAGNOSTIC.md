# T-0483 R35 — Stopped Sol full-ancestor ACL diagnostic (source only)

## Change

The inert stopped-worker candidate can now pin a supplied stage path from the local volume root through every ancestor using read-only handles. The chain rejects relative, UNC, verbatim, reparse and noncanonical paths. It checks the opened drive root against the mount manager's volume GUID root and compares the final opened stage object with the supplied pinned root using 128-bit file IDs. A bounded ACL scan reads each ancestor's owner and potentially write-capable allow ACE trustees, refuses unsupported ACEs, and compares the trustees twice with the original exited worker's retained token, groups and restricting SIDs. Exited identity, account, Job and final private-root checks bracket the scan.

This is conservative diagnostic evidence only. It cannot revoke a previously copied writable handle, prove every process outside the Job stopped, or prevent later ACL changes. It returns no burn, staging, repository-use, review or Wake authority.

## Verification

- Focused Windows tests passed: full-chain fixture one, staging 13, lifecycle 32 with one ignored helper. The full root suite passed 1,234 main tests with 28 ignored. Strict workspace Clippy, formatting and diff checks passed. Independent read-only review found no blocker after volume-root and strong-ID checks were tightened.
- Fixture checks reject unsafe path spellings and a different final object; ACL cases distinguish read-only from write-capable allow ACEs.

Source commit `c3ae701b1b04ea2243061e395bf275867c3a98e6` was pushed. Immediate exact-SHA GitHub readback found zero check-runs; latest visible workflow run still targeted older `4e9a2e2`. No remote CI acceptance is claimed.

## Remaining gate

The fixed `CatDeskSolWorker` account and trusted primary-token provider are absent, so no positive fixed-account launch occurred. Outside-Job processes, copied writable handles, later ACL changes and durable burn-to-stage recovery remain unproven. A genuine GPT-6.1 Sol/medium deliverable, natural `review_ready` and exact Wake receipt have not occurred.
