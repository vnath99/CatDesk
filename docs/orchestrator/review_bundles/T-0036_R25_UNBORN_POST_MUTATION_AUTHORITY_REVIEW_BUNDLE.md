# T-0036-R25 Unborn Post-Mutation Authority Review Bundle

R25 adds one reusable bounded post-mutation verifier for the typed unborn-head
path. Before `PUSH_SKIPPED_NO_COMMITTED_HISTORY` is recorded, including on
partial recovery, it re-reads `remote.origin.url` and requires the exact policy
origin, then performs the existing same-trusted-gh scope-qualified repository
EXISTS proof. Journal stages alone are not authority.

Committed-head push behavior is unchanged. Unborn roots still perform zero push,
content, index, branch, checkout, or commit mutation. R14-R24 workspace,
tool, REST account/scope, policy, drift, journal, and origin controls remain
unchanged.

Local verification runs format, strict clippy, Cargo tests, and diff check. No
live external workspace, GitHub, browser, daemon, tunnel, Scheduler, release,
remote, push, or Git publication action occurred.

Host acceptance: isolated reviewed build/reload; `CONNECTED_VERIFIED`; one
fresh Bug Bounty preflight; confirm only on positive unborn/zero-history/ABSENT
evidence; verify exact origin and authoritative EXISTS readback; then proceed to
the separately reviewed registration/adoption sequence.
