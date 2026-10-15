# T-0483 R33 — Dormant fixed-account Sol launch path (source only)

## Change

The private prepared-worker lifecycle accepts a host-supplied restricted primary-token lease for the fixed `CatDeskSolWorker` account. It rechecks the fixed account and token identity and reads the source and copied token handle's granted rights so a query-only handle cannot gain stronger rights through duplication. A bad token fails before the create-once launch reservation. With a valid lease, the dormant path reserves the exact task/turn/nonce/launch inputs, then uses `CreateProcessAsUserW` with a pinned executable, explicit arguments/environment/current directory, no inherited handles, suspended creation and a creation-time Job attribute. It checks the returned child's fixed token, Job membership and image before resume; uncertain failures retain bounded ATTENTION ownership.

This source has no trusted token provider, and this host does not have the fixed worker account. Its same-user negative fixture is not a positive fixed-account launch. No stage, burn, repository-use, review or Wake consumer was connected.

## Verification

- Focused token-security tests passed 10 with one ignored helper; fixed-account identity tests passed eight; lifecycle tests passed 32 with one ignored helper. Strict workspace Clippy, formatting and scoped diff checks passed. Independent source review found no blocking issue.
- The isolated root shell/cwd test and remaining all-features root Rust suite, including nested Wake, passed locally. Strict root/Wake Clippy and formatting, root all-bin build and diff checks passed on the settled source.

Source commit `3e869d3878c1386babe638e480e0638668286831` was pushed. Its immediate exact-SHA GitHub readback found zero workflow runs and zero check-runs; a later read-only check still found zero check-runs. No remote CI acceptance is claimed.

## Remaining gate

A separately approved token provider and positive distinct-user launch are required. The retained Job does not prove absence of escaped descendants, duplicated outside handles or ancestor write rights. Durable burn-to-stage handoff, genuine GPT-6.1 Sol/medium deliverable, natural `review_ready` and exact Wake receipt remain open. The stopped candidate grants no stage or repository authority.
