# T-0060C-R2 Durable Promotion Recovery Review Bundle

## Repair

The reviewed-build promotion helper now creates one small, non-secret
transaction record under `.catdesk/promotion-recovery` before the first
canonical `catdesk.exe` or manifest mutation. The record contains only a
version, the incomplete-mutation phase, prior and candidate SHA-256 values,
and the candidate's workspace-relative path. It is written through a flushed
temporary file and then renamed into place.

On every later invocation, transaction detection runs before normal canonical
release validation. Plan mode is read-only and returns
`INTERRUPTED_RECOVERY_REQUIRED`. Execute mode independently validates the
journal and bounded prior backup, restores and revalidates the prior
binary/manifest pair, and only clears the transaction after the pair and any
needed existing PID-scoped candidate-to-canonical handoff are proven.

The sole recorded phase deliberately means canonical mutation was started but
not durably completed. Thus a full candidate pair with that transaction is
still rolled back to the known prior release. Unknown schemas/phases, malformed
or oversized records, mismatched or incomplete backups, and reparse ambiguity
fail closed and retain evidence for an operator; no broad cleanup occurs.

## Deterministic coverage

The PowerShell fixture creates prior-process-death states in a temporary
workspace and proves recovery for binary-new/manifest-old, full candidate pair
with unfinished transaction, and prior pair already restored. It also covers
read-only interrupted plan mode, idempotent next invocation, malformed and
unknown journals, bad hashes, incomplete backup, recovery handoff failure,
reparse-path ambiguity, and ordinary successful promotion.

## Boundaries and remaining risk

The helper preserves the existing two-stage PID-scoped handoff and one
binary/manifest backup. It does not inspect credentials, routes, browser or
tunnel state, nor does it start, stop, or reconnect externally owned Secure
MCP infrastructure. No live promotion, daemon reload, task action, browser
action, tunnel action, or Git publication is part of this repair. CatDesk-host
acceptance and authoritative diff capture remain pending.

## Local checks

`scripts/test-promote-reviewed-catdesk-build.ps1` passed together with the
existing lifecycle, autostart, bootstrap, and production-acceptance fixture
suites. `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, and `git diff --check` passed (with the workspace's pre-existing
CRLF conversion notices). The full `cargo test` run completed 449 passing and
11 ignored tests, with seven pre-existing host-only failures: three tests need
an unavailable local advisor executable and four hit Windows process-tree
cancellation access denial. No Rust source was changed for this repair.
