# T-0483 R26 — normal Sol completion and retained attention owner (source only)

## Scope

`src/delegated/sol_worker_lifecycle.rs` now distinguishes a normal worker completion from a forced abort. The private normal path waits for the exact opened child to exit with success and then requires the retained Job to show zero active processes and an empty PID list without first terminating it. A failed or timed-out run takes a bounded abort path and mints no normal drained witness.

Launch, pre-resume, bind, abort and drain failures return typed `Clean` only after exact child termination and Job empty are proven (or before any child exists). Uncertain cleanup returns a private `SolJobAttentionOwner` retaining the exact process/Job, prepared task/turn/nonce/output-root context and digest-pinned executable lease. A retry can only attempt bounded abort; it cannot turn attention into a deliverable or reissue the selected nonce. No runtime or repository-use consumer accepts these types.

## Evidence and limits

- Focused lifecycle tests: 20 passed, 1 ignored helper. They exercise zero and nonzero exits, timeout with injected Job termination failure, Job-accounting failure, prepared-run/image retention and attention retry. Independent read-only review found no concrete fail-open defect and identified one stale source comment, which was corrected. Combined gates passed: root all-features Rust workspace (main 1,212 passed/27 ignored plus integrations), independent Wake all-features Rust workspace (170 library tests plus bins/integrations), full Python `pytest.exe -q --tb=short`, strict root/Wake all-target/all-feature Clippy, both format checks, root all-bin build and diff check.
- This remains a same-user diagnostic `Command` launcher that creates a suspended child before assigning it to a Job. It has no approved executable digest source, fixed-account positive token, creation-time Job-list assignment or durable host-crash attention registry. A dropped in-memory owner can release the retained lease; the create-once nonce is not thereby reusable. Zero child exit and an empty Job do not prove Sol model/effort, native turn, output bytes, processes outside the Job, inherited-handle denial, ancestor ACL/lost-write authority or stage recovery.
- No real Sol task→verified deliverable→review_ready→natural Wake cycle was accepted. No nonce burn, staging or repository use was enabled.
- Published source commit `89ef5558655e77d3e0c0302cf4c5e0a33862a0ac` has zero GitHub workflow/check runs on immediate and bounded post-build readback. Local tests and the unsigned diagnostic BuildOnly are not independent CI or a live fixed-account worker acceptance.

## Next gate

Add a protected approved executable policy and typed launch plan, then a fixed-account `CreateProcessAsUserW` route with creation-time Job association and a durable attention registry. Prove lost write authority and nonce-burn-to-stage recovery before any output can reach repository staging or review.
