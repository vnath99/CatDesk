# T-0483 R22 — Private Windows Job drain mechanics for Sol worker (source only)

## Implemented

- A dormant private owner in `src/delegated/sol_worker_lifecycle.rs` launches a helper suspended, assigns it to an unnamed noninheritable Job with kill-on-close and no breakaway flags, checks exact Job membership, then resumes it. A consuming bounded `finish` terminates the retained Job, checks the original process handle and terminal status, and requires both zero active processes and an empty process-ID list from that same Job. Failed assignment/resume uses checked exact-handle termination with a bounded cleanup wait.
- The module returns only `Result<()>`. It creates no stopped-worker or repository-use capability, changes no Sol runtime route, account, nonce, stage or protected workspace.

## Verification and limits

- Four focused Windows lifecycle tests passed; one ignored helper runs only as a spawned test child. Tests cover membership before resume, descendant membership/drain, invalid deadline, and bounded suspended-child cleanup. Strict root Clippy passed. An independent read-only review found no fail-open path; its one unbounded cleanup concern was fixed and retested.
- The helper uses the current user for mechanics. The fixed `CatDeskSolWorker` account is absent locally. A future restricted launcher must bind its actual token, deny inherited writable handles, prove every stage/repository ancestor excludes worker write/rename/delete rights, and account for children that can escape the Job. Only a separate post-exit handoff may read stage bytes for repository use; R21's current readback still requires a live worker. Burn-to-stage recovery, a real Sol/medium deliverable, authentic review and natural Wake remain open.